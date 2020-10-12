use std::collections::{BTreeMap, HashMap};

use crate::extractor::MetaAttr;

use html5ever::{LocalName, Namespace, QualName};
use kuchiki::{
    iter::{Descendants, Elements, Select},
    traits::*,
    NodeData, NodeRef,
};
use regex::Regex;

const HTML_NS: &'static str = "http://www.w3.org/1999/xhtml";
const PHRASING_ELEMS: [&str; 39] = [
    "abbr", "audio", "b", "bdo", "br", "button", "cite", "code", "data", "datalist", "dfn", "em",
    "embed", "i", "img", "input", "kbd", "label", "mark", "math", "meter", "noscript", "object",
    "output", "progress", "q", "ruby", "samp", "script", "select", "small", "span", "strong",
    "sub", "sup", "textarea", "time", "var", "wbr",
];
mod regexes;

pub struct Readability {
    root_node: NodeRef,
    byline: Option<String>,
    article_title: String,
}

#[derive(Debug, PartialEq)]
struct SizeInfo {
    rows: usize,
    columns: usize,
}

impl Readability {
    pub fn new(html_str: &str) -> Self {
        Self {
            root_node: kuchiki::parse_html().one(html_str),
            byline: None,
            article_title: "".into(),
        }
    }
    pub fn parse(&mut self) {
        self.unwrap_no_script_tags();
        self.remove_scripts();
        self.prep_document();
        // TODO: Add implementation for get_article_metadata
    }

    /// Recursively check if node is image, or if node contains exactly only one image
    /// whether as a direct child or as its descendants.
    fn is_single_image(node_ref: &NodeRef) -> bool {
        if let Some(element) = node_ref.as_element() {
            if &element.name.local == "img" {
                return true;
            }
        }

        if node_ref.children().filter(Self::has_content).count() != 1
            || !node_ref.text_contents().trim().is_empty()
        {
            return false;
        }

        return Readability::is_single_image(
            &node_ref
                .children()
                .filter(Self::has_content)
                .next()
                .expect("Unable to get first child which should exist"),
        );
    }

    fn has_content(node_ref: &NodeRef) -> bool {
        match node_ref.data() {
            NodeData::Text(text) => !text.borrow().trim().is_empty(),
            _ => true,
        }
    }

    /// Find all <noscript> that are located after <img> nodes, and which contain only one <img> element.
    /// Replace the first image with the image from inside the <noscript> tag, and remove the <noscript> tag.
    /// This improves the quality of the images we use on some sites (e.g. Medium).
    fn unwrap_no_script_tags(&mut self) {
        if let Ok(imgs) = self.root_node.select("img") {
            imgs.filter(|img_node_ref| {
                let img_attrs = img_node_ref.attributes.borrow();
                !img_attrs.map.iter().any(|(name, attr)| {
                    // TODO: Replace with regex
                    &name.local == "src"
                        || &name.local == "srcset"
                        || &name.local == "data-src"
                        || &name.local == "data-srcset"
                        || attr.value.ends_with(".jpg")
                        || attr.value.ends_with(".jpeg")
                        || attr.value.ends_with(".png")
                        || attr.value.ends_with(".webp")
                })
            })
            .for_each(|img_ref| img_ref.as_node().detach());
        }

        if let Ok(noscripts) = self.root_node.select("noscript") {
            for noscript in noscripts {
                let inner_node_ref = kuchiki::parse_fragment(
                    QualName::new(None, Namespace::from(HTML_NS), LocalName::from("div")),
                    Vec::new(),
                )
                .one(noscript.text_contents());
                if !Self::is_single_image(&inner_node_ref) {
                    continue;
                }
                if let Some(mut prev_elem) = noscript.as_node().previous_sibling() {
                    // TODO: Fix this to have a better way of extracting nodes that are elements
                    while prev_elem.as_element().is_none() {
                        match prev_elem.previous_sibling() {
                            Some(new_prev) => prev_elem = new_prev,
                            None => break,
                        };
                    }

                    if Self::is_single_image(&prev_elem) && prev_elem.as_element().is_some() {
                        let prev_img = if &prev_elem.as_element().unwrap().name.local != "img" {
                            prev_elem.select_first("img").unwrap().as_node().clone()
                        } else {
                            prev_elem.clone()
                        };
                        let new_img = inner_node_ref.select_first("img").unwrap();
                        let prev_attrs = prev_img.as_element().unwrap().attributes.borrow();
                        let prev_attrs = prev_attrs.map.iter().filter(|(attr, val)| {
                            !val.value.trim().is_empty()
                                && (&attr.local == "src"
                                    || &attr.local == "srcset"
                                    // TODO: Replace with regex
                                    || val.value.ends_with(".jpg")
                                    || val.value.ends_with(".jpeg")
                                    || val.value.ends_with(".png")
                                    || val.value.ends_with(".webp"))
                        });
                        for (prev_attr, prev_value) in prev_attrs {
                            match new_img.attributes.borrow().get(&prev_attr.local) {
                                Some(value) => {
                                    if value == prev_value.value {
                                        continue;
                                    }
                                }
                                None => (),
                            }

                            let attr_name: &str = &prev_attr.local;
                            let mut attr_name = attr_name.to_owned();
                            if new_img.attributes.borrow().contains(attr_name.clone()) {
                                let new_name = format!("data-old-{}", &attr_name);
                                attr_name = new_name;
                            }
                            new_img
                                .attributes
                                .borrow_mut()
                                .insert(attr_name, prev_value.value.clone());
                        }

                        let inner_node_child = Self::next_element(inner_node_ref.first_child());
                        prev_elem.insert_after(inner_node_child.unwrap());
                        prev_elem.detach();
                    }
                }
            }
        }
    }

    /// Removes script tags from the document.
    fn remove_scripts(&mut self) {
        match self.root_node.select("script") {
            Ok(script_elems) => script_elems.for_each(|elem| elem.as_node().detach()),
            Err(_) => (),
        }
        match self.root_node.select("noscript") {
            Ok(noscript_elems) => noscript_elems.for_each(|elem| elem.as_node().detach()),
            Err(_) => (),
        }
    }

    /// Prepare the HTML document for readability to scrape it. This includes things like stripping
    /// CSS, and handling terrible markup.
    fn prep_document(&mut self) {
        match self.root_node.select("style") {
            Ok(style_elems) => style_elems.for_each(|elem| elem.as_node().detach()),
            Err(_) => (),
        }
        self.replace_brs();
        match self.root_node.select("font") {
            Ok(nodes_iter) => Self::replace_node_tags(nodes_iter, "span"),
            Err(_) => (),
        }
    }

    /// Replaces 2 or more successive <br> elements with a single <p>.
    /// Whitespace between <br> elements are ignored. For example:
    ///  <div>foo<br>bar<br> <br><br>abc</div>
    /// will become:
    ///   <div>foo<br>bar<p>abc</p></div>
    fn replace_brs(&mut self) {
        if let Ok(mut br_tags) = self.root_node.select("br") {
            while let Some(br_tag) = br_tags.next() {
                let mut next = Self::next_element(br_tag.as_node().next_sibling());
                let mut replaced = false;
                while let Some(next_elem) = next {
                    if next_elem.as_element().is_some()
                        && &next_elem.as_element().as_ref().unwrap().name.local == "br"
                    {
                        replaced = true;
                        let br_sibling = next_elem.next_sibling();
                        next_elem.detach();
                        next = Self::next_element(br_sibling);
                    } else {
                        break;
                    }
                }
                if replaced {
                    let p = NodeRef::new_element(
                        QualName::new(None, Namespace::from(HTML_NS), LocalName::from("p")),
                        BTreeMap::new(),
                    );
                    br_tag.as_node().insert_before(p);
                    let p = br_tag.as_node().previous_sibling().unwrap();
                    br_tag.as_node().detach();

                    next = p.next_sibling();
                    while next.is_some() {
                        let next_sibling = next.unwrap();
                        if let Some(next_elem) = next_sibling.as_element() {
                            if &next_elem.name.local == "br" {
                                if let Some(second_sibling) = next_sibling.next_sibling() {
                                    if second_sibling.as_element().is_some()
                                        && "br" == &second_sibling.as_element().unwrap().name.local
                                    {
                                        break;
                                    }
                                }
                            }
                        }

                        if !Self::is_phrasing_content(&next_sibling) {
                            break;
                        }

                        let sibling = next_sibling.next_sibling();
                        p.append(next_sibling);
                        next = sibling;
                    }

                    while let Some(first_child) = p.first_child() {
                        if Self::is_whitespace(&first_child) {
                            first_child.detach();
                        } else {
                            break;
                        }
                    }

                    while let Some(last_child) = p.last_child() {
                        if Self::is_whitespace(&last_child) {
                            last_child.detach();
                        } else {
                            break;
                        }
                    }

                    if let Some(parent) = p.parent() {
                        if &parent.as_element().as_ref().unwrap().name.local == "p" {
                            Self::set_node_tag(&parent, "div");
                        }
                    }
                }
            }
        }
    }

    /// Iterates over a Select, and calls set_node_tag for each node.
    fn replace_node_tags(nodes: Select<Elements<Descendants>>, name: &str) {
        for node in nodes {
            Self::set_node_tag(node.as_node(), name);
        }
    }

    /// Replaces the specified NodeRef by replacing its name. This works by copying over its
    /// children and its attributes.
    fn set_node_tag(node_ref: &NodeRef, name: &str) {
        // TODO: Change function to own node_ref so that a user does not try to use it after dropping
        match node_ref.as_element() {
            Some(elem) => {
                let attributes = elem.attributes.borrow().clone().map.into_iter();
                let replacement = NodeRef::new_element(
                    QualName::new(None, Namespace::from(HTML_NS), LocalName::from(name)),
                    attributes,
                );
                for child in node_ref.children() {
                    replacement.append(child);
                }
                node_ref.insert_before(replacement);
                node_ref.detach();
            }
            None => (),
        }
    }

    fn is_whitespace(node_ref: &NodeRef) -> bool {
        match node_ref.data() {
            NodeData::Element(elem_data) => &elem_data.name.local == "br",
            NodeData::Text(text_ref) => text_ref.borrow().trim().len() == 0,
            _ => false,
        }
    }

    /// Finds the next element, starting from the given node, and ignoring
    /// whitespace in between. If the given node is an element, the same node is
    /// returned.
    fn next_element(node_ref: Option<NodeRef>) -> Option<NodeRef> {
        // TODO: Could probably be refactored to use the elements method
        let mut node_ref = node_ref;
        while node_ref.is_some() {
            match node_ref.as_ref().unwrap().data() {
                NodeData::Element(_) => break,
                _ => {
                    if node_ref.as_ref().unwrap().text_contents().trim().is_empty() {
                        node_ref = node_ref.as_ref().unwrap().next_sibling();
                    } else {
                        break;
                    }
                }
            }
        }
        node_ref
    }

    /// Determine if a node qualifies as phrasing content.
    /// https://developer.mozilla.org/en-US/docs/Web/Guide/HTML/Content_categories#Phrasing_content
    fn is_phrasing_content(node_ref: &NodeRef) -> bool {
        node_ref.as_text().is_some()
            || match node_ref.as_element() {
                Some(elem) => {
                    let name: &str = &elem.name.local;
                    PHRASING_ELEMS.contains(&name)
                        || ((name == "a" || name == "del" || name == "ins")
                            && node_ref
                                .children()
                                .all(|child_ref| Self::is_phrasing_content(&child_ref)))
                }
                None => false,
            }
    }

    ///Attempts to get excerpt and byline metadata for the article. @return Object with optional "excerpt" and "byline" properties
    fn get_article_metadata(&self) -> MetaAttr {
        unimplemented!()
    }

    /// Converts an inline CSS string to a [HashMap] of property and value(s)
    fn inline_css_str_to_map(css_str: &str) -> HashMap<&str, &str> {
        css_str
            .split(";")
            .filter(|split_str| !split_str.trim().is_empty())
            .map(|str_pair| {
                let mut vals = str_pair.split(":");
                (vals.next().unwrap().trim(), vals.next().unwrap().trim())
            })
            .collect()
    }

    fn is_probably_visible(node_ref: &NodeRef) -> bool {
        if let Some(elem_data) = node_ref.as_element() {
            let attributes = elem_data.attributes.borrow();
            (if let Some(css_str) = attributes.get("style"){
                let style_map = Self::inline_css_str_to_map(css_str);
                if let Some(display_val) = style_map.get("display") {
                    display_val != &"hidden"
                } else {
                    true
                }
            } else {
                true
            })
                && !attributes.contains("hidden")
            // check for "fallback-image" so that wikimedia math images are displayed
                && (if let Some(aria_hidden_attr) = attributes.get("aria-hidden"){
                    aria_hidden_attr.trim() != "true"
                } else if let Some(class_str) = attributes.get("class"){
                    !class_str.split(" ").collect::<Vec<&str>>().contains(&"fallback-image")
                } else {
                    true
                })
        } else {
            // Technically, it should not matter what value is returned here
            true
        }
    }

    /// Check whether the input string could be a byline, i.e is less than 100 chars
    fn is_valid_byline(input: &str) -> bool {
        let text = input.trim();
        text.len() > 0 && text.len() < 100
    }

    fn check_byline(&mut self, node_ref: &NodeRef, match_string: &str) -> bool {
        if self.byline.is_none() {
            if let Some(elem_data) = node_ref.as_element() {
                let elem_attrs = elem_data.attributes.borrow();
                let rel_attr = elem_attrs.get("rel");
                let itemprop_attr = elem_attrs.get("itemprop");
                let byline_regex = Regex::new(r"(?i)byline|author|dateline|writtenby|p-author")
                    .expect("Unable to create byline_regex");
                let is_byline = (if rel_attr.is_some() {
                    rel_attr.unwrap() == "author"
                } else if itemprop_attr.is_some() {
                    itemprop_attr.unwrap().contains("author")
                } else {
                    byline_regex.is_match(match_string)
                }) && Self::is_valid_byline(&node_ref.text_contents());
                if is_byline {
                    self.byline = Some(node_ref.text_contents().trim().to_owned());
                }
                dbg!(is_byline);
                is_byline
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Traverse the DOM from node to node, starting at the node passed in.
    /// Pass true for the second parameter to indicate this node itself
    /// (and its kids) are going away, and we want the next node over.
    ///
    /// Calling this in a loop will traverse the DOM depth-first.
    fn get_next_node(node_ref: &NodeRef, ignore_self_and_kids: bool) -> Option<NodeRef> {
        let has_elem_children = node_ref.children().elements().count();
        if !ignore_self_and_kids && has_elem_children > 0 {
            Self::next_element(node_ref.first_child())
        } else if let Some(next_sibling) = Self::next_element(node_ref.next_sibling()) {
            Some(next_sibling)
        } else {
            // Keep walking up the node hierarchy until a parent with element siblings is found
            let mut node = node_ref.parent();
            while let Some(parent) = node {
                if let Some(next_sibling) = Self::next_element(parent.next_sibling()) {
                    return Some(next_sibling);
                } else {
                    node = parent.parent();
                }
            }
            None
        }
    }

    /// Removes the node_ref passed in and returns the next possible node by calling [get_next_node]
    fn remove_and_get_next(node_ref: NodeRef) -> Option<NodeRef> {
        let next_node = Self::get_next_node(&node_ref, true);
        node_ref.detach();
        next_node
    }

    /// Check if a given node has one of its ancestor tag name matching the
    /// provided one.
    fn has_ancestor_tag(
        node_ref: &NodeRef,
        tag_name: &str,
        max_depth: Option<i32>,
        filter_fn: Option<fn(&NodeRef) -> bool>,
    ) -> bool {
        let mut depth = 0;
        let max_depth = max_depth.or(Some(3)).unwrap();
        let mut parent = node_ref.parent();
        while parent.is_some() {
            let parent_node = parent.as_ref().unwrap();
            if parent_node.as_element().is_none() {
                // The recursion may go up the DOM tree upto a document node at which point it must stop
                return false;
            }
            let parent_node_elem = parent_node.as_element().unwrap();
            if max_depth > 0 && depth > max_depth {
                return false;
            }
            if &parent_node_elem.name.local == tag_name
                && (filter_fn.is_none() || filter_fn.unwrap()(parent_node))
            {
                return true;
            }
            parent = parent_node.parent();
            depth += 1;
        }
        false
    }

    fn is_element_without_content(node_ref: &NodeRef) -> bool {
        let child_count = node_ref.children().count();
        node_ref.as_element().is_some()
            && node_ref.text_contents().trim().is_empty()
            && (child_count == 0
                || child_count
                    == node_ref.select("br").unwrap().count()
                        + node_ref.select("hr").unwrap().count())
    }

    /// Check if this node has only whitespace and a single element with given tag
    /// Returns false if the <div> node contains non-empty text nodes
    /// or if it contains no element with given tag or more than 1 element.
    fn has_single_tag_inside_element(node_ref: &NodeRef, tag_name: &str) -> bool {
        let first_child = node_ref.children().elements().next();
        if node_ref.children().elements().count() != 1
            || (first_child.is_some() && &first_child.unwrap().name.local != tag_name)
        {
            return false;
        }
        !node_ref.children().any(|node| {
            node.as_text().is_some()
                && Regex::new(r"\S$")
                    .unwrap()
                    .is_match(&node.text_contents().trim_end())
        })
    }

    fn get_inner_text(node_ref: &NodeRef, normalize_spaces: Option<bool>) -> String {
        let will_normalize = normalize_spaces.unwrap_or(true);
        let text = node_ref.text_contents();
        let text = text.trim();
        let normalize_regex = Regex::new(r"\s{2,}").unwrap();
        if will_normalize {
            return normalize_regex.replace_all(&text, " ").to_string();
        }
        text.to_owned()
    }

    /// Get the density of links as a percentage of the content
    /// This is the amount of text that is inside a link divided by the total text in the node.
    fn get_link_density(node_ref: &NodeRef) -> f32 {
        let text_length = Self::get_inner_text(node_ref, None).len() as f32;
        if text_length == 0_f32 {
            return 0_f32;
        }
        node_ref
            .select("a")
            .unwrap()
            .map(|a_node| Self::get_inner_text(a_node.as_node(), None).len() as f32)
            .sum::<f32>()
            / text_length
    }

    /// Determine whether element has any children block level elements.
    fn has_child_block_element(node_ref: &NodeRef) -> bool {
        // TODO: Refer to a static HashSet
        let block_level_elems: [&str; 32] = [
            "address",
            "article",
            "aside",
            "blockquote",
            "details",
            "dialog",
            "dd",
            "div",
            "dl",
            "dt",
            "fieldset",
            "figcaption",
            "footer",
            "form",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "header",
            "hgroup",
            "hr",
            "li",
            "main",
            "nav",
            "ol",
            "p",
            "pre",
            "section",
            "table",
            "ul",
        ];
        node_ref.children().any(|child_node| {
            if child_node.as_element().is_some() {
                let child_elem = child_node.as_element().unwrap();
                block_level_elems.contains(&&*child_elem.name.local)
                    || Self::has_child_block_element(&child_node)
            } else {
                false
            }
        })
    }

    /// Returns a [Vec] of ancestors
    fn get_node_ancestors(node_ref: &NodeRef, max_depth: Option<usize>) -> Vec<NodeRef> {
        node_ref.ancestors().take(max_depth.unwrap_or(1)).collect()
    }

    /// Get an element's class/id weight using regular expressions to tell if this
    /// element looks good or bad.
    fn get_class_weight(node_ref: &NodeRef) -> i32 {
        //TODO: Add check for weighing classes
        let mut weight = 0;
        let positive_regex = Regex::new(r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story").unwrap();
        let negative_regex = Regex::new(r"(?i)hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|foot|footer|footnote|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|tool|widget").unwrap();
        let node_elem = node_ref.as_element().unwrap();
        let node_attrs = node_elem.attributes.borrow();
        if let Some(id) = node_attrs.get("id") {
            if !id.trim().is_empty() {
                weight = if positive_regex.is_match(id) {
                    weight + 25
                } else if negative_regex.is_match(id) {
                    weight - 25
                } else {
                    weight
                }
            }
        }
        if let Some(class) = node_attrs.get("class") {
            if !class.trim().is_empty() {
                weight = if positive_regex.is_match(class) {
                    weight + 25
                } else if negative_regex.is_match(class) {
                    weight - 25
                } else {
                    weight
                }
            }
        }
        weight
    }

    /// Initialize a node with the readability attribute. Also checks the
    /// className/id for special names to add to its score.
    fn initialize_node(node_ref: &mut NodeRef) {
        if let Some(element) = node_ref.as_element() {
            let mut score = 0;
            // This must be computed first because it borrows the NodeRef which
            // should not also be mutably borrowed
            score += Self::get_class_weight(node_ref);
            let mut elem_attrs = element.attributes.borrow_mut();
            elem_attrs.insert("readability-score", score.to_string());
            let readability = elem_attrs.get_mut("readability-score");
            match &*element.name.local {
                "div" => score += 5,
                "pre" | "td" | "blockquote" => score += 3,
                "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => score -= 3,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => score -= 5,
                _ => (),
            }
            if let Some(x) = readability {
                *x = score.to_string();
            }
        }
    }

    fn get_row_and_column_count(node_ref: &NodeRef) -> SizeInfo {
        let mut rows = 0;
        let mut columns = 0;
        if let Ok(trs) = node_ref.select("tr") {
            for tr in trs {
                let tr_node = tr.as_node();
                let tr_attr = tr.attributes.borrow();
                let rowspan = tr_attr
                    .get("rowspan")
                    .map(|x| {
                        x.parse::<usize>()
                            .expect("Unable to parse rowspan value to usize")
                    })
                    .unwrap_or(1);
                rows += rowspan;
                let mut columns_in_row = 0;
                if let Ok(cells) = tr_node.select("td") {
                    for cell in cells {
                        let cell_attr = cell.attributes.borrow();
                        let colspan = cell_attr
                            .get("colspan")
                            .map(|x| {
                                x.parse::<usize>()
                                    .expect("Unable to parse colspan value to usize")
                            })
                            .unwrap_or(1);
                        columns_in_row += colspan;
                    }
                }
                columns = columns.max(columns_in_row);
            }
        }
        SizeInfo { rows, columns }
    }

    /// Look for 'data' (as opposed to 'layout') tables, for which we use similar checks as
    /// https://dxr.mozilla.org/mozilla-central/rev/71224049c0b52ab190564d3ea0eab089a159a4cf/accessible/html/HTMLTableAccessible.cpp#920
    fn mark_data_tables(&mut self) {
        if let Ok(tables) = self.root_node.select("table") {
            for table in tables {
                let mut table_attr = table.attributes.borrow_mut();
                let table_node = table.as_node();
                if table_attr.get("role") == Some("presentation") {
                    table_attr.insert("readability-data-table", "false".to_string());
                    continue;
                }
                if table_attr.get("datatable") == Some("0") {
                    table_attr.insert("readability-data-table", "false".to_string());
                    continue;
                }

                if table_attr.contains("summary") {
                    table_attr.insert("readability-data-table", "true".to_string());
                    continue;
                }
                if let Ok(caption) = table_node.select_first("caption") {
                    if caption.as_node().children().count() > 0 {
                        table_attr.insert("readability-data-table", "true".to_string());
                        continue;
                    }
                }
                let data_table_descendants = vec!["col", "colgroup", "tfoot", "thead", "th"];
                if data_table_descendants
                    .iter()
                    .any(|tag_name| table_node.select_first(tag_name).is_ok())
                {
                    table_attr.insert("readability-data-table", "true".to_string());
                    continue;
                }

                if table_node.select("table").unwrap().count() > 1 {
                    table_attr.insert("readability-data-table", "false".to_string());
                    continue;
                }

                let size_info = Self::get_row_and_column_count(table_node);
                if size_info.rows >= 10 || size_info.columns > 4 {
                    table_attr.insert("readability-data-table", "true".to_string());
                    continue;
                }

                if (size_info.rows * size_info.columns) > 10 {
                    table_attr.insert("readability-data-table", "true".to_string());
                    continue;
                } else {
                    table_attr.insert("readability-data-table", "false".to_string());
                    continue;
                }
            }
        }
    }

    /// Convert images and figures that have properties like data-src into images that can be loaded without JS
    fn fix_lazy_images(node_ref: &mut NodeRef) {
        let imgs = node_ref.select("img").unwrap();
        let pictures = node_ref.select("picture").unwrap();
        let figures = node_ref.select("figure").unwrap();
        let regex = Regex::new(r"(?i)^data:\s*([^\s;,]+)\s*;\s*base64\s*").unwrap();
        let nodes = imgs.chain(pictures).chain(figures);
        for node in nodes {
            let mut node_attr = node.attributes.borrow_mut();
            if let Some(src) = node_attr.get("src") {
                let src_captures = regex.captures(src);
                if src_captures.is_some() {
                    let svg_capture = src_captures.unwrap().get(1);
                    if svg_capture.is_some() && svg_capture.unwrap().as_str() == "image/svg+xml" {
                        continue;
                    }

                    let svg_could_be_removed = node_attr
                        .map
                        .iter()
                        .filter(|(name, _)| &name.local != "src")
                        .filter(|(_, val)| {
                            let regex = Regex::new(r"(?i)\.(jpg|jpeg|png|webp)").unwrap();
                            regex.is_match(&val.value)
                        })
                        .count()
                        > 0;

                    if svg_could_be_removed {
                        let base64_regex = Regex::new(r"(?i)base64\s*").unwrap();
                        let b64_start = base64_regex.find(src).unwrap().start();
                        let b64_length = src.len() - b64_start;
                        if b64_length < 133 {
                            node_attr.remove("src");
                        }
                    }
                }
            }
            let src = node_attr.get("src");
            let srcset = node_attr.get("srcset");
            let class = node_attr.get("class");
            if (src.is_some() || (srcset.is_some() && srcset.unwrap() != "null"))
                && class.is_some()
                && !class.unwrap().contains("lazy")
            {
                continue;
            }

            node_attr
                .map
                .clone()
                .iter()
                .filter(|(key, _)| !(&key.local == "src" || &key.local == "srcset"))
                .for_each(|(_, val)| {
                    let mut copy_to = "";
                    let srcset_regex = Regex::new(r"\.(jpg|jpeg|png|webp)\s+\d").unwrap();
                    let src_regex = Regex::new(r"^\s*\S+\.(jpg|jpeg|png|webp)\S*\s*$").unwrap();
                    if srcset_regex.is_match(&val.value) {
                        copy_to = "srcset";
                    } else if src_regex.is_match(&val.value) {
                        copy_to = "src";
                    }
                    if copy_to.len() > 0 {
                        let tag_name = &node.name.local;
                        if tag_name == "img" || tag_name == "picture" {
                            if let Some(attr) = node_attr.get_mut(copy_to) {
                                *attr = val.value.clone();
                            }
                        } else if tag_name == "figure" {
                            let node_ref = node.as_node();
                            let imgs = node_ref.select("img").unwrap();
                            let pictures = node_ref.select("picture").unwrap();
                            if imgs.chain(pictures).count() > 0 {
                                let img = NodeRef::new_element(
                                    QualName::new(
                                        None,
                                        Namespace::from(HTML_NS),
                                        LocalName::from("img"),
                                    ),
                                    BTreeMap::new(),
                                );
                                {
                                    let mut img_attr =
                                        img.as_element().unwrap().attributes.borrow_mut();
                                    img_attr.insert(copy_to, val.value.clone());
                                }
                                node_ref.append(img);
                            }
                        }
                    }
                });
        }
    }

    /// Clean an element of all tags of type "tag" if they look fishy. "Fishy" is an algorithm
    /// based on content length, classnames, link density, number of images & embeds, etc.
    fn clean_conditionally(node_ref: &mut NodeRef, tag_name: &str) {
        // TODO: Add flag check
        let is_list = tag_name == "ul" || tag_name == "ol";
        let mut nodes = node_ref.select(tag_name).unwrap();
        let is_data_table = |node_ref: &NodeRef| {
            let node_elem = node_ref.as_element().unwrap();
            let attrs = node_elem.attributes.borrow();
            !(attrs.get("readability-data-table") == Some("true"))
        };
        let get_char_count = |node_ref: &NodeRef| node_ref.text_contents().matches(",").count();
        let node_name = &node_ref.as_element().unwrap().name.local;
        // Because select returns an inclusive iterator, we should skip the first one.
        if node_name == tag_name {
            nodes.next();
        }
        nodes
            // Do not remove data tables
            .filter(|node_data_ref| {
                !(node_name == "table" && is_data_table(node_data_ref.as_node()))
            })
            // Do not remove if it is a child of a data table
            .filter(|node_data_ref| {
                !Self::has_ancestor_tag(
                    node_data_ref.as_node(),
                    tag_name,
                    Some(-1),
                    Some(is_data_table),
                )
            })
            .map(|node_data_ref|{
                let weight =  Self::get_class_weight(node_data_ref.as_node());
                (node_data_ref,weight)
            })
            .filter(|(_, weight)| weight < &0)
            .filter(|(node_data_ref,_)| get_char_count(node_data_ref.as_node()) < 10)
            .filter(|(node_data_ref,_)|{
                let embed_tags = vec!["object", "embed", "iframe"];
                let mut embeds = node_data_ref
                    .as_node()
                    .select(embed_tags.join(",").as_str())
                    .unwrap();
                if embed_tags.contains(&&*node_data_ref.name.local) {
                    embeds.next();
                }
                let videos_regex = Regex::new(r"(?i)\/\/(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)").unwrap();
                !(embeds.any(|node| &node.name.local == "object") ||  embeds.any(|node_data_ref| {
                         let attrs = node_data_ref.attributes.borrow();
                         !attrs.map.iter().any(|(key,_)|videos_regex.is_match(&key.local))
                     }))
            })
            .for_each(|(node_data_ref, weight)| {
                let node = node_data_ref.as_node();

                let mut p_nodes = node_data_ref.as_node().select("p").unwrap().count();
                let mut img_nodes = node_data_ref.as_node().select("img").unwrap().count();
                let mut li_nodes = node_data_ref.as_node().select("li").unwrap().count();
                let mut input_nodes = node_data_ref.as_node().select("input").unwrap().count();

                match node_name.as_ref() {
                    "p" => p_nodes -= 1,
                    "img" =>img_nodes -= 1,
                    "li" => li_nodes -= 1,
                    "input" => input_nodes -= 1,
                    _ => ()
                }

                let p = p_nodes as f32;
                let img = img_nodes as f32;

                let embed_count = node.select("object, embed, iframe").unwrap().count();
                let link_density = Self::get_link_density(node);
                let content_length = Self::get_inner_text(node, None).len();
                let has_figure_ancestor = Self::has_ancestor_tag(node, "figure", None, None);
                let have_to_remove = (img_nodes > 1 && p /img < 0.5 && !has_figure_ancestor) ||
                    (!is_list && li_nodes > p_nodes) || (input_nodes > (p_nodes / 3)) ||
                    (!is_list && content_length < 25 && (img_nodes == 0 || img_nodes > 2) && !has_figure_ancestor) ||
                    (!is_list && weight < 25 && link_density > 0.2) || (weight >= 25 && link_density > 0.5) ||
                    ((embed_count == 1 && content_length < 75) || embed_count > 1);
                if have_to_remove {
                    node.detach();
                }
            });
    }

    /// Clean a node of all elements of type "tag". (Unless it's a YouTube or Vimeo video)
    fn clean(node_ref: &mut NodeRef, tag_name: &str) {
        let is_embed = vec!["object", "embed", "iframe"].contains(&tag_name);
        let mut nodes = node_ref.select(tag_name).unwrap();
        let videos_regex = Regex::new(r"(?i)\/\/(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)").unwrap();
        if &node_ref.as_element().unwrap().name.local == tag_name {
            nodes.next();
        }
        nodes
            .filter(|node_data_ref| {
                !is_embed
                    || {
                        let attrs = node_data_ref.attributes.borrow();
                        !attrs
                            .map
                            .iter()
                            .any(|(key, _)| videos_regex.is_match(&key.local))
                    }
                    || &node_data_ref.name.local == "object"
            })
            .for_each(|node_data_ref| node_data_ref.as_node().detach());
    }

    /// Clean out spurious headers from an Element. Checks things like classnames and link density.
    fn clean_headers(node_ref: &mut NodeRef) {
        let mut nodes = node_ref.select("h1,h2").unwrap();

        if vec!["h1", "h2"].contains(&node_ref.as_element().unwrap().name.local.as_ref()) {
            nodes.next();
        }
        nodes
            .filter(|node_data_ref| Self::get_class_weight(node_data_ref.as_node()) < 0)
            .for_each(|node_data_ref| node_data_ref.as_node().detach());
    }

    /// Remove the style attribute on every element and descendants.
    fn clean_styles(node_ref: &mut NodeRef) {
        let presentational_attributes = vec![
            "align",
            "background",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "frame",
            "hspace",
            "rules",
            "style",
            "valign",
            "vspace",
        ];
        let deprecated_size_attribute_elems = vec!["table", "th", "td", "hr", "pre"];
        node_ref
            .inclusive_descendants()
            .elements()
            .filter(|node| &node.name.local != "svg")
            .for_each(|node_data_ref| {
                let mut attrs = node_data_ref.attributes.borrow_mut();
                presentational_attributes.iter().for_each(|pres_attr| {
                    attrs.remove(*pres_attr);
                });
                if deprecated_size_attribute_elems.contains(&node_data_ref.name.local.as_ref()) {
                    attrs.remove("width");
                    attrs.remove("height");
                }
            });
    }

    /// Clean out elements that match the specified conditions
    fn clean_matched_nodes(node_ref: &mut NodeRef, filter_fn: impl Fn(&NodeRef, &str) -> bool) {
        let end_of_search_marker_node = Self::get_next_node(node_ref, true);
        let mut next_node = Self::get_next_node(node_ref, false);
        while next_node.is_some() && next_node != end_of_search_marker_node {
            let node = next_node.unwrap();
            let attrs = node.as_element().unwrap().attributes.borrow();
            let class = attrs.get("class").unwrap_or("");
            let id = attrs.get("id").unwrap_or("");
            if filter_fn(&node, &(class.to_string() + " " + id)) {
                next_node = Self::remove_and_get_next(node.clone());
            } else {
                next_node = Self::get_next_node(&node, false);
            }
        }
    }

    /// Prepare the article node for display. Clean out any inline styles, iframes,
    /// forms, strip extraneous <p> tags, etc.
    fn prep_article(&mut self, node_ref: &mut NodeRef) {
        Self::clean_styles(node_ref);
        Self::fix_lazy_images(node_ref);
        Self::clean_conditionally(node_ref, "form");
        Self::clean_conditionally(node_ref, "fieldset");
        Self::clean(node_ref, "object");
        Self::clean(node_ref, "h1");
        Self::clean(node_ref, "footer");
        Self::clean(node_ref, "link");
        Self::clean(node_ref, "aside");

        // TODO: Extract as constant
        let share_element_threshold = 500;
        let regex = Regex::new(r"(\b|_)(share|sharedaddy)(\b|_)").unwrap();

        node_ref.children().for_each(|mut node| {
            Self::clean_matched_nodes(&mut node, |node: &NodeRef, match_string| {
                regex.is_match(match_string) && node.text_contents().len() < share_element_threshold
            });
        });

        let mut h2 = node_ref.select("h2").unwrap();
        if h2.by_ref().count() == 1 {
            let h2_node = h2.next().unwrap();
            let length_similar_rate = ((h2_node.text_contents().len() - self.article_title.len())
                as f32)
                / self.article_title.len() as f32;
            if length_similar_rate.abs() < 0.5 {
                let titles_match = if length_similar_rate > 0.0 {
                    h2_node.text_contents().contains(&self.article_title)
                } else {
                    self.article_title.contains(&h2_node.text_contents())
                };
                if titles_match {
                    Self::clean(node_ref, "h2");
                }
            }
        }

        Self::clean(node_ref, "iframe");
        Self::clean(node_ref, "input");
        Self::clean(node_ref, "textarea");
        Self::clean(node_ref, "select");
        Self::clean(node_ref, "button");
        Self::clean_headers(node_ref);

        Self::clean_conditionally(node_ref, "table");
        Self::clean_conditionally(node_ref, "ul");
        Self::clean_conditionally(node_ref, "div");

        node_ref
            .select("p")
            .unwrap()
            .filter(|node_data_ref| {
                let p_node = node_data_ref.as_node();
                let img_count = p_node.select("img").unwrap().count();
                let embed_count = p_node.select("embed").unwrap().count();
                let object_count = p_node.select("object").unwrap().count();
                let iframe_count = p_node.select("iframe").unwrap().count();
                let total = img_count + embed_count + object_count + iframe_count;
                total == 0 && Self::get_inner_text(node_data_ref.as_node(), Some(false)).is_empty()
            })
            .for_each(|node_data_ref| node_data_ref.as_node().detach());

        node_ref
            .select("br")
            .unwrap()
            .filter(|node_data_ref| {
                let br_node = node_data_ref.as_node();
                let next_node = Self::next_element(br_node.next_sibling());
                next_node.is_some() && &next_node.unwrap().as_element().unwrap().name.local == "p"
            })
            .for_each(|node_data_ref| node_data_ref.as_node().detach());

        node_ref.select("table").unwrap().for_each(|node_data_ref| {
            let table_node = node_data_ref.as_node();
            let table_child = Self::next_element(table_node.first_child());
            let tbody = if Self::has_single_tag_inside_element(&table_node, "tbody") {
                table_child.as_ref().unwrap()
            } else {
                table_node
            };

            if Self::has_single_tag_inside_element(&tbody, "tr") {
                let row = Self::next_element(tbody.first_child()).unwrap();
                if Self::has_single_tag_inside_element(&row, "td") {
                    let cell = Self::next_element(row.first_child()).unwrap();
                    let tag = if cell
                        .children()
                        .all(|cell_child| Self::is_phrasing_content(&cell_child))
                    {
                        "p"
                    } else {
                        "div"
                    };
                    Self::set_node_tag(&cell, tag);
                }
            }
        });
    }

    /// Using a variety of metrics (content score, classname, element types), find the content that is most likely to be the stuff
    /// a user wants to read. Then return it wrapped up in a div.
    fn grab_article(&mut self) {
        // var doc = this._doc;
        // var isPaging = (page !== null ? true: false);
        // page = page ? page : this._doc.body;

        // // We can't grab an article if we don't have a page!
        // if (!page) {
        //   this.log("No body found in document. Abort.");
        //   return null;
        // }

        // var pageCacheHtml = page.innerHTML;

        // while (true) {
        //   var stripUnlikelyCandidates = this._flagIsActive(this.FLAG_STRIP_UNLIKELYS);

        //   // First, node prepping. Trash nodes that look cruddy (like ones with the
        //   // class name "comment", etc), and turn divs into P tags where they have been
        //   // used inappropriately (as in, where they contain no other block level elements.)
        //   var elementsToScore = [];
        //   var node = this._doc.documentElement;

        //   while (node) {
        //     var matchString = node.className + " " + node.id;

        //     if (!this._isProbablyVisible(node)) {
        //       this.log("Removing hidden node - " + matchString);
        //       node = this._removeAndGetNext(node);
        //       continue;
        //     }

        //     // Check to see if this node is a byline, and remove it if it is.
        //     if (this._checkByline(node, matchString)) {
        //       node = this._removeAndGetNext(node);
        //       continue;
        //     }

        //     // Remove unlikely candidates
        //     if (stripUnlikelyCandidates) {
        //       if (this.REGEXPS.unlikelyCandidates.test(matchString) &&
        //           !this.REGEXPS.okMaybeItsACandidate.test(matchString) &&
        //           !this._hasAncestorTag(node, "table") &&
        //           node.tagName !== "BODY" &&
        //           node.tagName !== "A") {
        //         this.log("Removing unlikely candidate - " + matchString);
        //         node = this._removeAndGetNext(node);
        //         continue;
        //       }

        //       if (node.getAttribute("role") == "complementary") {
        //         this.log("Removing complementary content - " + matchString);
        //         node = this._removeAndGetNext(node);
        //         continue;
        //       }
        //     }

        //     // Remove DIV, SECTION, and HEADER nodes without any content(e.g. text, image, video, or iframe).
        //     if ((node.tagName === "DIV" || node.tagName === "SECTION" || node.tagName === "HEADER" ||
        //          node.tagName === "H1" || node.tagName === "H2" || node.tagName === "H3" ||
        //          node.tagName === "H4" || node.tagName === "H5" || node.tagName === "H6") &&
        //         this._isElementWithoutContent(node)) {
        //       node = this._removeAndGetNext(node);
        //       continue;
        //     }

        //     if (this.DEFAULT_TAGS_TO_SCORE.indexOf(node.tagName) !== -1) {
        //       elementsToScore.push(node);
        //     }

        //     // Turn all divs that don't have children block level elements into p's
        //     if (node.tagName === "DIV") {
        //       // Put phrasing content into paragraphs.
        //       var p = null;
        //       var childNode = node.firstChild;
        //       while (childNode) {
        //         var nextSibling = childNode.nextSibling;
        //         if (this._isPhrasingContent(childNode)) {
        //           if (p !== null) {
        //             p.appendChild(childNode);
        //           } else if (!this._isWhitespace(childNode)) {
        //             p = doc.createElement("p");
        //             node.replaceChild(p, childNode);
        //             p.appendChild(childNode);
        //           }
        //         } else if (p !== null) {
        //           while (p.lastChild && this._isWhitespace(p.lastChild)) {
        //             p.removeChild(p.lastChild);
        //           }
        //           p = null;
        //         }
        //         childNode = nextSibling;
        //       }

        //       // Sites like http://mobile.slate.com encloses each paragraph with a DIV
        //       // element. DIVs with only a P element inside and no text content can be
        //       // safely converted into plain P elements to avoid confusing the scoring
        //       // algorithm with DIVs with are, in practice, paragraphs.
        //       if (this._hasSingleTagInsideElement(node, "P") && this._getLinkDensity(node) < 0.25) {
        //         var newNode = node.children[0];
        //         node.parentNode.replaceChild(newNode, node);
        //         node = newNode;
        //         elementsToScore.push(node);
        //       } else if (!this._hasChildBlockElement(node)) {
        //         node = this._setNodeTag(node, "P");
        //         elementsToScore.push(node);
        //       }
        //     }
        //     node = this._getNextNode(node);
        //   }

        //   /**
        //    * Loop through all paragraphs, and assign a score to them based on how content-y they look.
        //    * Then add their score to their parent node.
        //    *
        //    * A score is determined by things like number of commas, class names, etc. Maybe eventually link density.
        //   **/
        //   var candidates = [];
        //   this._forEachNode(elementsToScore, function(elementToScore) {
        //     if (!elementToScore.parentNode || typeof(elementToScore.parentNode.tagName) === "undefined")
        //       return;

        //     // If this paragraph is less than 25 characters, don't even count it.
        //     var innerText = this._getInnerText(elementToScore);
        //     if (innerText.length < 25)
        //       return;

        //     // Exclude nodes with no ancestor.
        //     var ancestors = this._getNodeAncestors(elementToScore, 3);
        //     if (ancestors.length === 0)
        //       return;

        //     var contentScore = 0;

        //     // Add a point for the paragraph itself as a base.
        //     contentScore += 1;

        //     // Add points for any commas within this paragraph.
        //     contentScore += innerText.split(",").length;

        //     // For every 100 characters in this paragraph, add another point. Up to 3 points.
        //     contentScore += Math.min(Math.floor(innerText.length / 100), 3);

        //     // Initialize and score ancestors.
        //     this._forEachNode(ancestors, function(ancestor, level) {
        //       if (!ancestor.tagName || !ancestor.parentNode || typeof(ancestor.parentNode.tagName) === "undefined")
        //         return;

        //       if (typeof(ancestor.readability) === "undefined") {
        //         this._initializeNode(ancestor);
        //         candidates.push(ancestor);
        //       }

        //       // Node score divider:
        //       // - parent:             1 (no division)
        //       // - grandparent:        2
        //       // - great grandparent+: ancestor level * 3
        //       if (level === 0)
        //         var scoreDivider = 1;
        //       else if (level === 1)
        //         scoreDivider = 2;
        //       else
        //         scoreDivider = level * 3;
        //       ancestor.readability.contentScore += contentScore / scoreDivider;
        //     });
        //   });

        //// I think the section here could be most explicitly written using a call to sort and then accessing
        //// the first 5 elements. Alternatively, it can still just as well be done with a reduce/fold function
        //   // After we've calculated scores, loop through all of the possible
        //   // candidate nodes we found and find the one with the highest score.
        //   var topCandidates = [];
        //   for (var c = 0, cl = candidates.length; c < cl; c += 1) {
        //     var candidate = candidates[c];

        //     // Scale the final candidates score based on link density. Good content
        //     // should have a relatively small link density (5% or less) and be mostly
        //     // unaffected by this operation.
        //     var candidateScore = candidate.readability.contentScore * (1 - this._getLinkDensity(candidate));
        //     candidate.readability.contentScore = candidateScore;

        //     this.log("Candidate:", candidate, "with score " + candidateScore);

        //     for (var t = 0; t < this._nbTopCandidates; t++) {
        //       var aTopCandidate = topCandidates[t];

        //       if (!aTopCandidate || candidateScore > aTopCandidate.readability.contentScore) {
        //         topCandidates.splice(t, 0, candidate);
        //         if (topCandidates.length > this._nbTopCandidates)
        //           topCandidates.pop();
        //         break;
        //       }
        //     }
        //   }

        //   var topCandidate = topCandidates[0] || null;
        //   var neededToCreateTopCandidate = false;
        //   var parentOfTopCandidate;

        //   // If we still have no top candidate, just use the body as a last resort.
        //   // We also have to copy the body node so it is something we can modify.
        //   if (topCandidate === null || topCandidate.tagName === "BODY") {
        //     // Move all of the page's children into topCandidate
        //     topCandidate = doc.createElement("DIV");
        //     neededToCreateTopCandidate = true;
        //     // Move everything (not just elements, also text nodes etc.) into the container
        //     // so we even include text directly in the body:
        //     var kids = page.childNodes;
        //     while (kids.length) {
        //       this.log("Moving child out:", kids[0]);
        //       topCandidate.appendChild(kids[0]);
        //     }

        //     page.appendChild(topCandidate);

        //     this._initializeNode(topCandidate);
        //   } else if (topCandidate) {
        //     // Find a better top candidate node if it contains (at least three) nodes which belong to `topCandidates` array
        //     // and whose scores are quite closed with current `topCandidate` node.
        //     var alternativeCandidateAncestors = [];
        //     for (var i = 1; i < topCandidates.length; i++) {
        //       if (topCandidates[i].readability.contentScore / topCandidate.readability.contentScore >= 0.75) {
        //         alternativeCandidateAncestors.push(this._getNodeAncestors(topCandidates[i]));
        //       }
        //     }
        //     var MINIMUM_TOPCANDIDATES = 3;
        //     if (alternativeCandidateAncestors.length >= MINIMUM_TOPCANDIDATES) {
        //       parentOfTopCandidate = topCandidate.parentNode;
        //       while (parentOfTopCandidate.tagName !== "BODY") {
        //         var listsContainingThisAncestor = 0;
        //         for (var ancestorIndex = 0; ancestorIndex < alternativeCandidateAncestors.length && listsContainingThisAncestor < MINIMUM_TOPCANDIDATES; ancestorIndex++) {
        //           listsContainingThisAncestor += Number(alternativeCandidateAncestors[ancestorIndex].includes(parentOfTopCandidate));
        //         }
        //         if (listsContainingThisAncestor >= MINIMUM_TOPCANDIDATES) {
        //           topCandidate = parentOfTopCandidate;
        //           break;
        //         }
        //         parentOfTopCandidate = parentOfTopCandidate.parentNode;
        //       }
        //     }
        //     if (!topCandidate.readability) {
        //       this._initializeNode(topCandidate);
        //     }

        //     // Because of our bonus system, parents of candidates might have scores
        //     // themselves. They get half of the node. There won't be nodes with higher
        //     // scores than our topCandidate, but if we see the score going *up* in the first
        //     // few steps up the tree, that's a decent sign that there might be more content
        //     // lurking in other places that we want to unify in. The sibling stuff
        //     // below does some of that - but only if we've looked high enough up the DOM
        //     // tree.
        //     parentOfTopCandidate = topCandidate.parentNode;
        //     var lastScore = topCandidate.readability.contentScore;
        //     // The scores shouldn't get too low.
        //     var scoreThreshold = lastScore / 3;
        //     while (parentOfTopCandidate.tagName !== "BODY") {
        //       if (!parentOfTopCandidate.readability) {
        //         parentOfTopCandidate = parentOfTopCandidate.parentNode;
        //         continue;
        //       }
        //       var parentScore = parentOfTopCandidate.readability.contentScore;
        //       if (parentScore < scoreThreshold)
        //         break;
        //       if (parentScore > lastScore) {
        //         // Alright! We found a better parent to use.
        //         topCandidate = parentOfTopCandidate;
        //         break;
        //       }
        //       lastScore = parentOfTopCandidate.readability.contentScore;
        //       parentOfTopCandidate = parentOfTopCandidate.parentNode;
        //     }

        //     // If the top candidate is the only child, use parent instead. This will help sibling
        //     // joining logic when adjacent content is actually located in parent's sibling node.
        //     parentOfTopCandidate = topCandidate.parentNode;
        //     while (parentOfTopCandidate.tagName != "BODY" && parentOfTopCandidate.children.length == 1) {
        //       topCandidate = parentOfTopCandidate;
        //       parentOfTopCandidate = topCandidate.parentNode;
        //     }
        //     if (!topCandidate.readability) {
        //       this._initializeNode(topCandidate);
        //     }
        //   }

        //   // Now that we have the top candidate, look through its siblings for content
        //   // that might also be related. Things like preambles, content split by ads
        //   // that we removed, etc.
        //   var articleContent = doc.createElement("DIV");
        //   if (isPaging)
        //     articleContent.id = "readability-content";

        //   var siblingScoreThreshold = Math.max(10, topCandidate.readability.contentScore * 0.2);
        //   // Keep potential top candidate's parent node to try to get text direction of it later.
        //   parentOfTopCandidate = topCandidate.parentNode;
        //   var siblings = parentOfTopCandidate.children;

        //   for (var s = 0, sl = siblings.length; s < sl; s++) {
        //     var sibling = siblings[s];
        //     var append = false;

        //     this.log("Looking at sibling node:", sibling, sibling.readability ? ("with score " + sibling.readability.contentScore) : "");
        //     this.log("Sibling has score", sibling.readability ? sibling.readability.contentScore : "Unknown");

        //     if (sibling === topCandidate) {
        //       append = true;
        //     } else {
        //       var contentBonus = 0;

        //       // Give a bonus if sibling nodes and top candidates have the example same classname
        //       if (sibling.className === topCandidate.className && topCandidate.className !== "")
        //         contentBonus += topCandidate.readability.contentScore * 0.2;

        //       if (sibling.readability &&
        //           ((sibling.readability.contentScore + contentBonus) >= siblingScoreThreshold)) {
        //         append = true;
        //       } else if (sibling.nodeName === "P") {
        //         var linkDensity = this._getLinkDensity(sibling);
        //         var nodeContent = this._getInnerText(sibling);
        //         var nodeLength = nodeContent.length;

        //         if (nodeLength > 80 && linkDensity < 0.25) {
        //           append = true;
        //         } else if (nodeLength < 80 && nodeLength > 0 && linkDensity === 0 &&
        //                    nodeContent.search(/\.( |$)/) !== -1) {
        //           append = true;
        //         }
        //       }
        //     }

        //     if (append) {
        //       this.log("Appending node:", sibling);

        //       if (this.ALTER_TO_DIV_EXCEPTIONS.indexOf(sibling.nodeName) === -1) {
        //         // We have a node that isn't a common block level element, like a form or td tag.
        //         // Turn it into a div so it doesn't get filtered out later by accident.
        //         this.log("Altering sibling:", sibling, "to div.");

        //         sibling = this._setNodeTag(sibling, "DIV");
        //       }

        //       articleContent.appendChild(sibling);
        //       // siblings is a reference to the children array, and
        //       // sibling is removed from the array when we call appendChild().
        //       // As a result, we must revisit this index since the nodes
        //       // have been shifted.
        //       s -= 1;
        //       sl -= 1;
        //     }
        //   }

        //   if (this._debug)
        //     this.log("Article content pre-prep: " + articleContent.innerHTML);
        //   // So we have all of the content that we need. Now we clean it up for presentation.
        //   this._prepArticle(articleContent);
        //   if (this._debug)
        //     this.log("Article content post-prep: " + articleContent.innerHTML);

        //   if (neededToCreateTopCandidate) {
        //     // We already created a fake div thing, and there wouldn't have been any siblings left
        //     // for the previous loop, so there's no point trying to create a new div, and then
        //     // move all the children over. Just assign IDs and class names here. No need to append
        //     // because that already happened anyway.
        //     topCandidate.id = "readability-page-1";
        //     topCandidate.className = "page";
        //   } else {
        //     var div = doc.createElement("DIV");
        //     div.id = "readability-page-1";
        //     div.className = "page";
        //     var children = articleContent.childNodes;
        //     while (children.length) {
        //       div.appendChild(children[0]);
        //     }
        //     articleContent.appendChild(div);
        //   }

        //   if (this._debug)
        //     this.log("Article content after paging: " + articleContent.innerHTML);

        //   var parseSuccessful = true;

        //   // Now that we've gone through the full algorithm, check to see if
        //   // we got any meaningful content. If we didn't, we may need to re-run
        //   // grabArticle with different flags set. This gives us a higher likelihood of
        //   // finding the content, and the sieve approach gives us a higher likelihood of
        //   // finding the -right- content.
        //   var textLength = this._getInnerText(articleContent, true).length;
        //   if (textLength < this._charThreshold) {
        //     parseSuccessful = false;
        //     page.innerHTML = pageCacheHtml;

        //     if (this._flagIsActive(this.FLAG_STRIP_UNLIKELYS)) {
        //       this._removeFlag(this.FLAG_STRIP_UNLIKELYS);
        //       this._attempts.push({articleContent: articleContent, textLength: textLength});
        //     } else if (this._flagIsActive(this.FLAG_WEIGHT_CLASSES)) {
        //       this._removeFlag(this.FLAG_WEIGHT_CLASSES);
        //       this._attempts.push({articleContent: articleContent, textLength: textLength});
        //     } else if (this._flagIsActive(this.FLAG_CLEAN_CONDITIONALLY)) {
        //       this._removeFlag(this.FLAG_CLEAN_CONDITIONALLY);
        //       this._attempts.push({articleContent: articleContent, textLength: textLength});
        //     } else {
        //       this._attempts.push({articleContent: articleContent, textLength: textLength});
        //       // No luck after removing flags, just return the longest text we found during the different loops
        //       this._attempts.sort(function (a, b) {
        //         return b.textLength - a.textLength;
        //       });

        //       // But first check if we actually have something
        //       if (!this._attempts[0].textLength) {
        //         return null;
        //       }

        //       articleContent = this._attempts[0].articleContent;
        //       parseSuccessful = true;
        //     }
        //   }

        //   if (parseSuccessful) {
        //     // Find out text direction from ancestors of final top candidate.
        //     var ancestors = [parentOfTopCandidate, topCandidate].concat(this._getNodeAncestors(parentOfTopCandidate));
        //     this._someNode(ancestors, function(ancestor) {
        //       if (!ancestor.tagName)
        //         return false;
        //       var articleDir = ancestor.getAttribute("dir");
        //       if (articleDir) {
        //         this._articleDir = articleDir;
        //         return true;
        //       }
        //       return false;
        //     });
        //     return articleContent;
        //   }
        // }
    }
}

#[cfg(test)]
mod test {
    use super::{Readability, SizeInfo, HTML_NS};
    use html5ever::{LocalName, Namespace, QualName};
    use kuchiki::traits::*;
    use kuchiki::NodeRef;

    // TODO: Refactor not to use test file possibly
    const TEST_HTML: &'static str = include_str!("../../test_html/simple.html");

    #[test]
    fn test_unwrap_no_script_tags() {
        let mut readability = Readability::new(TEST_HTML);
        let img_count = readability.root_node.select("img").unwrap().count();
        assert_eq!(3, img_count);
        readability.unwrap_no_script_tags();
        let img_count = readability.root_node.select("img").unwrap().count();
        assert_eq!(2, img_count);

        // Ensure attributes were copied over
        let updated_img = readability.root_node.select_first("img#lazy-load").unwrap();
        let updated_img_attrs = updated_img.attributes.borrow();
        assert_eq!(true, updated_img_attrs.contains("data-old-src"));
        assert_eq!(Some("lazy-load.png"), updated_img_attrs.get("data-old-src"));
        assert_eq!(Some("eager-load.png"), updated_img_attrs.get("src"));
    }

    #[test]
    fn test_is_single_image() {
        let readability = Readability::new(TEST_HTML);

        let img_elem_ref = readability.root_node.select_first("img").unwrap();
        assert_eq!(true, Readability::is_single_image(&img_elem_ref.as_node()));

        let noscript_elem_ref = readability.root_node.select_first("noscript").unwrap();
        assert_eq!(
            false,
            Readability::is_single_image(&noscript_elem_ref.as_node())
        );

        let div_elem_ref = readability
            .root_node
            .select_first("div.invalid-elems")
            .unwrap();
        assert_eq!(false, Readability::is_single_image(&div_elem_ref.as_node()));

        let div_elem_ref = kuchiki::parse_fragment(
            QualName::new(None, Namespace::from(HTML_NS), LocalName::from("div")),
            Vec::new(),
        )
        .one(noscript_elem_ref.as_node().text_contents().trim());

        assert_eq!(true, Readability::is_single_image(&div_elem_ref));
    }

    #[test]
    fn test_remove_scripts() {
        let mut readability = Readability::new(TEST_HTML);

        let noscript_elems = readability.root_node.select("noscript").unwrap();
        assert_eq!(1, noscript_elems.count());
        readability.remove_scripts();
        let noscript_elems = readability.root_node.select("noscript").unwrap();
        assert_eq!(0, noscript_elems.count());
    }

    #[test]
    fn test_next_element() {
        let html_str = r#"
         <p id="a">This is a node</p>
         <!-- Commented content  -->
         <p id="b">This is another node. The next line is just whitespace</p>

         This is standalone text"#;
        let doc = Readability::new(html_str);
        let p = doc.root_node.select_first("#a").unwrap();
        let p = p.as_node();
        let mut p_node_option: Option<NodeRef> = Some(p.clone());
        p_node_option = Readability::next_element(p_node_option);
        assert_eq!(Some(p.clone()), p_node_option);

        let p_node_option = p_node_option.unwrap();
        let p_node_option = p_node_option.as_element();
        let p_node_option_attr = p_node_option.unwrap().attributes.borrow();
        assert_eq!("a", p_node_option_attr.get("id").unwrap());

        let next = Readability::next_element(p.next_sibling());

        let next = next.unwrap();
        let next_elem = next.as_element();
        let next_attr = next_elem.unwrap().attributes.borrow();
        assert_eq!("b", next_attr.get("id").unwrap());

        let next = Readability::next_element(next.next_sibling());

        let next = next.unwrap();
        assert_eq!(true, next.as_text().is_some());
        assert_eq!("This is standalone text", next.text_contents().trim());

        let next = Readability::next_element(None);
        assert_eq!(None, next);
    }

    #[test]
    fn test_is_phrasing_content() {
        let html_str = r#"
        Some text node
        <b>This is a phrasing content node</b>
        <p>This is not a phrasing content node</p>
        <a href="\#"><i>This is also a phrasing content</i></a>
        <a href="\#"><p>This is not a phrasing content</p></a>
        "#;
        let doc = Readability::new(html_str);
        let body = doc.root_node.select_first("body").unwrap();
        let body = body.as_node();
        let mut body_children = body.children();
        let mut node = body_children.next().unwrap();
        assert_eq!(true, node.as_text().is_some());
        assert_eq!(true, Readability::is_phrasing_content(&node));

        node = node.next_sibling().unwrap();
        assert_eq!("b", &node.as_element().unwrap().name.local);
        assert_eq!(true, Readability::is_phrasing_content(&node));

        node = node.next_sibling().unwrap(); // Skips the text node from the new line character
        node = node.next_sibling().unwrap();
        assert_eq!("p", &node.as_element().unwrap().name.local);
        assert_eq!(false, Readability::is_phrasing_content(&node));

        node = node.next_sibling().unwrap(); // Skips the text node from the new line character
        node = node.next_sibling().unwrap();
        assert_eq!("a", &node.as_element().unwrap().name.local);
        assert_eq!(true, Readability::is_phrasing_content(&node));

        node = node.next_sibling().unwrap(); // Skips the text node from the new line character
        node = node.next_sibling().unwrap();
        assert_eq!("a", &node.as_element().unwrap().name.local);
        assert_eq!(false, Readability::is_phrasing_content(&node));
    }

    #[test]
    fn test_is_whitespace() {
        let html_str = r#"
        <p>Definitely not whitespace</p>
        I am also not whitespace
        <p>     </p>
        <br>
        "#;
        let doc = Readability::new(html_str);
        let body = doc.root_node.select_first("body").unwrap();

        let mut node = body.as_node().first_child().unwrap();
        assert_eq!("p", &node.as_element().unwrap().name.local);
        assert_eq!(false, Readability::is_whitespace(&node));

        node = node.next_sibling().unwrap();
        assert_eq!(true, node.as_text().is_some());
        assert_eq!(false, Readability::is_whitespace(&node));

        node = node.next_sibling().unwrap();
        assert_eq!("p", &node.as_element().unwrap().name.local);
        assert_eq!(
            true,
            Readability::is_whitespace(&node.first_child().unwrap())
        );

        // This is testing the new line character in between the <p> and <br> tags
        node = node.next_sibling().unwrap();
        assert_eq!(true, node.as_text().is_some());
        assert_eq!(true, Readability::is_whitespace(&node));

        node = node.next_sibling().unwrap();
        assert_eq!("br", &node.as_element().unwrap().name.local);
        assert_eq!(true, Readability::is_whitespace(&node));
    }

    #[test]
    fn test_set_node_tag() {
        let html_str = r#"
        <div id="target" class="some random class" tabindex="0"><p>Child 1</p><p>Child 2</p></div>
        <div id="not-the-target">The div above is being replaced</div>
        "#;
        let doc = Readability::new(html_str);
        let target = doc.root_node.select_first("#target").unwrap();
        let children_count = doc.root_node.children().count();
        let target_children_count = target.as_node().children().count();

        assert_eq!("div", &target.name.local);
        Readability::set_node_tag(&target.as_node(), "section");

        assert_eq!(children_count, doc.root_node.children().count());
        let target = doc.root_node.select_first("#target").unwrap();
        assert_eq!("section", &target.name.local);
        assert_eq!(target_children_count, target.as_node().children().count());

        let target_attrs = target.as_node().as_element().unwrap().attributes.borrow();
        assert_eq!(3, target_attrs.map.len());

        let old_div = doc.root_node.select_first("div#target");
        assert_eq!(true, old_div.is_err());
    }

    #[test]
    fn test_replace_node_tags() {
        let html_str = r#"
        <div id="replace-p">
          <p>Tag 1</p><p>Tag 2</p><p>Tag 3</p>
        </div>
        "#;
        let doc = Readability::new(html_str);
        let target_parent = doc.root_node.select_first("div#replace-p").unwrap();
        let target_parent_child_count = target_parent.as_node().children().count();
        let nodes = target_parent.as_node().select("p").unwrap();

        Readability::replace_node_tags(nodes, "span");
        assert_eq!(
            target_parent_child_count,
            target_parent.as_node().children().count()
        );

        let nodes = target_parent.as_node().select("p").unwrap();
        assert_eq!(0, nodes.count());
        let nodes = target_parent.as_node().select("span").unwrap();
        assert_eq!(3, nodes.count());
    }

    #[test]
    fn test_replace_brs() {
        let html_str = r#"
        <div>foo<br>bar<br> <br><br>abc</div>
        "#;
        let mut doc = Readability::new(html_str);
        let div = doc.root_node.select_first("div").unwrap();
        let br_count = div.as_node().select("br").unwrap().count();
        let p_count = div.as_node().select("p").unwrap().count();
        assert_eq!(4, br_count);
        assert_eq!(0, p_count);

        doc.replace_brs();
        let br_count = div.as_node().select("br").unwrap().count();
        let p_count = div.as_node().select("p").unwrap().count();
        assert_eq!(1, br_count);
        assert_eq!(1, p_count);

        let p_node = div.as_node().select_first("p").unwrap();
        assert_eq!("abc", p_node.as_node().text_contents());

        let html_str = r#"
        <p>foo<br>bar<br> <br><br>abc</p>
        "#;
        doc = Readability::new(html_str);
        let p = doc.root_node.select_first("p").unwrap();
        let div_count = doc.root_node.select("div").unwrap().count();
        let br_count = p.as_node().select("br").unwrap().count();
        assert_eq!(4, br_count);
        assert_eq!(0, div_count);

        doc.replace_brs();
        let br_count = doc.root_node.select("br").unwrap().count();
        let div_count = doc.root_node.select("div").unwrap().count();
        let p_count = doc.root_node.select("p").unwrap().count();
        assert_eq!(1, br_count);
        assert_eq!(1, div_count);
        assert_eq!(1, p_count);
        let p_node = doc.root_node.select_first("p").unwrap();
        assert_eq!("abc", p_node.as_node().text_contents());
    }

    #[test]
    fn test_prep_document() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <head>
            <style>div {padding: 20px; border-bottom: 2px solid black; }</style>
          </head>
          <body>
            <font face="Times New Roman" size="10">Times New Roman</font>
            <div>foo<br>bar<br> <br><br>abc</div>
          </body>
        </html>
        "#;
        let mut doc = Readability::new(html_str);
        doc.prep_document();

        let style_nodes = doc.root_node.select("style").unwrap();
        let font_nodes = doc.root_node.select("font").unwrap();
        let p_nodes = doc.root_node.select("p").unwrap();
        let br_nodes = doc.root_node.select("br").unwrap();
        assert_eq!(0, style_nodes.count());
        assert_eq!(0, font_nodes.count());
        assert_eq!(1, p_nodes.count());
        assert_eq!(1, br_nodes.count());
    }

    #[test]
    fn test_inline_css_str_to_map() {
        use std::collections::HashMap;
        let css_str = "display: flex; height: 200px; width: 250px; justify-content: center; align-items: center; border: 2px solid black";
        let mut css_map = HashMap::new();
        css_map.insert("display", "flex");
        css_map.insert("height", "200px");
        css_map.insert("width", "250px");
        css_map.insert("justify-content", "center");
        css_map.insert("align-items", "center");
        css_map.insert("border", "2px solid black");

        let css_str_to_vec = Readability::inline_css_str_to_map(css_str);
        assert_eq!(css_map, css_str_to_vec);
        let mut css_map = HashMap::new();
        css_map.insert("color", "red");
        assert_eq!(css_map, Readability::inline_css_str_to_map("color: red;"));
    }

    #[test]
    fn test_is_probably_visible() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p id="visible">Lorem ipsum dolores</p>
            <div id="hidden-div" style="display: hidden">
              <p>This is hidden and so is the parent</p>
            </div>
            <input value="Some good CSRF token" hidden>
            <div id="hidden-aria" style="display: flex;" aria-hidden="true">
              <p>This is not considered visible</p>
            </div>
            <div id="visible-aria" style="display: flex;" aria-hidden="false">
              <p>This is considered visible</p>
            </div>
            <img src="./some-img.png" class="fallback-image">
            <div id="visible-div" style="display: block" class="visible" aria-hidden="false">
              <p>This is fully visible</p>
            </div>
          </body>
        </html>
      "#;
        let doc = Readability::new(html_str);
        let div_node = doc.root_node.select_first("div#hidden-div").unwrap();
        let p_node = doc.root_node.select_first("p#visible").unwrap();
        let input_node = doc.root_node.select_first("input").unwrap();
        let hidden_aria_div_node = doc.root_node.select_first("div#hidden-aria").unwrap();
        let visible_aria_div_node = doc.root_node.select_first("div#visible-aria").unwrap();
        let img_node = doc.root_node.select_first("img").unwrap();
        let visible_div_node = doc.root_node.select_first("div#visible-div").unwrap();
        assert_eq!(true, Readability::is_probably_visible(&p_node.as_node()));
        assert_eq!(false, Readability::is_probably_visible(&div_node.as_node()));
        assert_eq!(
            false,
            Readability::is_probably_visible(&input_node.as_node())
        );
        assert_eq!(
            false,
            Readability::is_probably_visible(&hidden_aria_div_node.as_node())
        );
        assert_eq!(
            true,
            Readability::is_probably_visible(&visible_aria_div_node.as_node())
        );
        assert_eq!(false, Readability::is_probably_visible(&img_node.as_node()));
        assert_eq!(
            true,
            Readability::is_probably_visible(&visible_div_node.as_node())
        );
    }

    #[test]
    fn test_check_byline() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
        <body>
          <p class="byline description" id="author">
This test is used to find out whether a given node is a byline. This works by checking whether
a node has a rel attribute with "author" as its value, or if "author"
is part of its value in the itemprop attribute. If neither is the case then it checks whether the classes and id
of the node match a regex of a potential byline. If any condition is met, then the content must be less than 100
characters. For that reason, this <p> tag could not be a byline because it's too long.
          </p>
          <p class="author">A Paperoni maintainer</p>
          <p class="authors not-byline"></p>
          <p rel="author">Maintainer of Paperoni</p>
        </body>
        </html>
        "#;
        let mut doc = Readability::new(html_str);
        assert_eq!(&None, &doc.byline);
        let p1_node = doc.root_node.select_first("p.byline").unwrap();
        let p2_node = doc.root_node.select_first("p.author").unwrap();
        let p3_node = doc.root_node.select_first("p.not-byline").unwrap();
        let p4_node = doc.root_node.select_first(r#"p[rel="author""#).unwrap();
        assert_eq!(
            false,
            doc.check_byline(p1_node.as_node(), "byline description author")
        );
        assert_eq!(true, doc.check_byline(p2_node.as_node(), "author"));
        assert_eq!(
            false,
            doc.check_byline(p3_node.as_node(), "authors not-byline")
        );
        assert_eq!(Some("A Paperoni maintainer".into()), doc.byline);
        // The test below is false because there is already an existing byline.
        assert_eq!(false, doc.check_byline(p4_node.as_node(), ""));
    }

    #[test]
    fn test_get_next_node() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <div id="body-child-1">
              <p id="start">Foobar content</p>
              <div id="start-sib">
                <span>First child</span>
              </div>
            </div>
            <div id="body-child-2"><span>This will not be reached</p></div>
            <p id="body-child-last">Last element</p>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let node = doc.root_node.select_first("p#start").unwrap();
        let next_node = Readability::get_next_node(node.as_node(), false);
        assert_eq!(true, next_node.is_some());
        let next_node = next_node.unwrap();
        let next_node_attr = next_node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("start-sib"), next_node_attr.get("id"));

        let next_node = Readability::get_next_node(&next_node, false);
        assert_eq!(true, next_node.is_some());
        let next_node = next_node.unwrap();
        assert_eq!("span", &next_node.as_element().unwrap().name.local);

        let next_node = Readability::get_next_node(&next_node, false);
        assert_eq!(true, next_node.is_some());
        let next_node = next_node.unwrap();
        let next_node_attr = next_node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("body-child-2"), next_node_attr.get("id"));

        let next_node = Readability::get_next_node(&next_node, true);
        assert_eq!(true, next_node.is_some());
        let next_node = next_node.unwrap();
        let next_node_attr = next_node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("body-child-last"), next_node_attr.get("id"));

        let next_node = Readability::get_next_node(&next_node, true);
        assert_eq!(None, next_node);
    }

    #[test]
    fn test_remove_and_get_next() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <div id="body-child-1">
              <p id="start">Foobar content</p>
              <div id="start-sib">
                <span>First child</span>
              </div>
            </div>
            <div id="body-child-2"><span>This will not be reached</p></div>
            <p id="body-child-last">Last element</p>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let node = doc.root_node.select_first("div#body-child-1").unwrap();
        let p_node = Readability::get_next_node(node.as_node(), false).unwrap();
        let next_node = Readability::remove_and_get_next(p_node);
        assert_eq!(true, next_node.is_some());

        let next_node = next_node.unwrap();
        let next_node_attr = next_node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("start-sib"), next_node_attr.get("id"));

        // Confirm the p node no longer exists
        let p_node = doc.root_node.select_first("p#start");
        assert_eq!(true, p_node.is_err());
    }

    #[test]
    fn test_has_ancestor_tag() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <div>
              <main>
                <p>
                  <span>Target node</span>
                </p>
              </main>
            </div>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let target = doc.root_node.select_first("span").unwrap();
        assert_eq!(
            true,
            Readability::has_ancestor_tag(target.as_node(), "div", None, None)
        );
        assert_eq!(
            false,
            Readability::has_ancestor_tag(target.as_node(), "div", Some(1), None)
        );
        assert_eq!(
            false,
            Readability::has_ancestor_tag(
                target.as_node(),
                "div",
                Some(5),
                Some(|node_ref| {
                    let node_attrs = node_ref.as_element().unwrap().attributes.borrow();
                    node_attrs.contains("class")
                })
            )
        );
    }

    #[test]
    fn test_is_element_without_content() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p>Node with content</p><!-- A comment node which is regarded as not having content -->
            <p id="empty"></p>
            <div id="contentful">
              <p>
                <span>Target node</span>
              </p>
            </div>
            <div id="no-content"><br><br><br><br><br><br><hr><hr><br></div>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let target = doc.root_node.select_first("p").unwrap();
        assert_eq!(
            false,
            Readability::is_element_without_content(target.as_node())
        );

        let target = target.as_node().next_sibling().unwrap();
        assert_eq!(true, target.as_comment().is_some());
        assert_eq!(false, Readability::is_element_without_content(&target));

        let mut target = doc.root_node.select_first("p#empty").unwrap();
        assert_eq!(
            true,
            Readability::is_element_without_content(target.as_node())
        );

        target = doc.root_node.select_first("div#contentful").unwrap();
        assert_eq!(
            false,
            Readability::is_element_without_content(target.as_node())
        );

        target = doc.root_node.select_first("div#no-content").unwrap();
        assert_eq!(
            true,
            Readability::is_element_without_content(target.as_node())
        );
    }

    #[test]
    fn test_has_single_tag_inside_element() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p id="one">No element tags here</p>
            <p id="two"><span>The p tag has only one tag</span></p>
            <p id="three">
              <span>Target node</span>
              <span>
                The parent has multiple children
              </span>
            </p>
            <p id="four">
              The text here means this div doesn't have a single tag
              <span>Target node</span>
            </p>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("p#one").unwrap();
        assert_eq!(
            false,
            Readability::has_single_tag_inside_element(target.as_node(), "span")
        );

        target = doc.root_node.select_first("p#two").unwrap();
        assert_eq!(
            true,
            Readability::has_single_tag_inside_element(target.as_node(), "span")
        );

        target = doc.root_node.select_first("p#three").unwrap();
        assert_eq!(
            false,
            Readability::has_single_tag_inside_element(target.as_node(), "span")
        );

        target = doc.root_node.select_first("p#four").unwrap();
        assert_eq!(
            false,
            Readability::has_single_tag_inside_element(target.as_node(), "span")
        );
    }

    #[test]
    fn test_get_inner_text() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p>The quick brown fox jumps       over the lazy dog</p>
           </body>
        </html>
         "#;
        let doc = Readability::new(html_str);
        let target = doc.root_node.select_first("p").unwrap();
        assert_eq!(
            49,
            Readability::get_inner_text(target.as_node(), Some(false)).len()
        );
        assert_eq!(
            43,
            Readability::get_inner_text(target.as_node(), None).len()
        );
    }

    #[test]
    fn test_get_link_density() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p id="one">Zero link density</p>
            <p id="two">Link density > 0 <a href="https://www.rust-lang.org/">The Rust home page</a></p>
            <p id="three"><a></a><a></a></p>
           </body>
        </html>
         "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("p#one").unwrap();
        assert_eq!(0_f32, Readability::get_link_density(target.as_node()));

        target = doc.root_node.select_first("p#two").unwrap();
        assert_eq!(
            18_f32 / 35_f32,
            Readability::get_link_density(target.as_node())
        );

        target = doc.root_node.select_first("p#three").unwrap();
        assert_eq!(0_f32, Readability::get_link_density(target.as_node()));
    }

    #[test]
    fn test_has_child_block_element() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <p id="one">Has no <span>block level</span> elements</p>
            <p id="two">Link density > 0 <a href="https://www.rust-lang.org/">The Rust home page</a></p>
            <div id="three">
              <p>This is a block level element</p>
            </div>
           </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("p#one").unwrap();
        assert_eq!(
            false,
            Readability::has_child_block_element(target.as_node())
        );

        target = doc.root_node.select_first("p#two").unwrap();
        assert_eq!(
            false,
            Readability::has_child_block_element(target.as_node())
        );

        target = doc.root_node.select_first("div#three").unwrap();
        assert_eq!(true, Readability::has_child_block_element(target.as_node()));
    }

    #[test]
    fn test_get_node_ancestors() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <section>
              <div>
                <p><span></span></p>
              </div>
            </section>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("span").unwrap();
        assert_eq!(
            1,
            Readability::get_node_ancestors(target.as_node(), None).len()
        );
        assert_eq!(
            3,
            Readability::get_node_ancestors(target.as_node(), Some(3)).len()
        );
        assert_eq!(
            5,
            Readability::get_node_ancestors(target.as_node(), Some(5)).len()
        );
        assert_eq!(
            6,
            Readability::get_node_ancestors(target.as_node(), Some(200)).len()
        );

        target = doc.root_node.select_first("html").unwrap();
        assert_eq!(
            1,
            Readability::get_node_ancestors(target.as_node(), Some(4)).len()
        );
    }

    #[test]
    fn test_get_class_weight() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <div id="blog" class="main">
              <h1 class="hidden">Up next...</h1>
              <p id="story">A story is told...</p>
            </div>
            <div id="comments">
              Tell us what you think
              <p class="comment">Great read...</p>
            </div>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("body").unwrap();
        assert_eq!(0, Readability::get_class_weight(target.as_node()));

        target = doc.root_node.select_first("div#blog").unwrap();
        assert_eq!(50, Readability::get_class_weight(target.as_node()));

        target = doc.root_node.select_first("h1.hidden").unwrap();
        assert_eq!(-25, Readability::get_class_weight(target.as_node()));

        target = doc.root_node.select_first("p#story").unwrap();
        assert_eq!(25, Readability::get_class_weight(target.as_node()));

        target = doc.root_node.select_first("div#comments").unwrap();
        assert_eq!(-25, Readability::get_class_weight(target.as_node()));

        target = doc.root_node.select_first("p.comment").unwrap();
        assert_eq!(-25, Readability::get_class_weight(target.as_node()));
    }

    #[test]
    fn test_initialize_node() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <div id="blog" class="main">
              <h1 class="hidden">Up next...</h1>
              <p id="story">A story is told...</p>
            </div>
            <div id="comments">
              Tell us what you think
              <pre class="comment">Great read...</pre>
            </div>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let mut target = doc.root_node.select_first("div#blog").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("55"), node_attrs.get("readability-score"));

        target = doc.root_node.select_first("h1.hidden").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-30"), node_attrs.get("readability-score"));

        target = doc.root_node.select_first("p#story").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("25"), node_attrs.get("readability-score"));

        target = doc.root_node.select_first("div#comments").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-20"), node_attrs.get("readability-score"));

        target = doc.root_node.select_first("pre.comment").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-22"), node_attrs.get("readability-score"));
    }

    #[test]
    fn test_get_row_and_column_count() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <table>
              <tbody>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td rowspan="2">&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td colspan="2">&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td colspan="4">&nbsp;</td>
                </tr>
              </tbody>
            </table>
          </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let target = doc.root_node.select_first("table").unwrap();
        assert_eq!(
            SizeInfo {
                rows: 6,
                columns: 4
            },
            Readability::get_row_and_column_count(target.as_node())
        );
    }

    #[test]
    fn test_mark_data_tables() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
          <body>
            <table id="one"></table>
            <table width="100%" border="0" id="two">
              <tr valign="top">
                <td width="20%">Left</td>
                <td height="200" width="60%">Main</td>
                <td width="20%">Right</td>
              </tr>
            </table>
            <table id="three">
              <caption>Monthly savings</caption>
              <tr>
                <th>Month</th>
                <th>Savings</th>
              </tr>
              <tr>
                <td>January</td>
                <td>$100</td>
              </tr>
              <tr>
                <td>February</td>
                <td>$50</td>
              </tr>
            </table>
            <table id="four">
              <tbody>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td rowspan="2">&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td colspan="2">&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                </tr>
                <tr>
                  <td colspan="4">&nbsp;</td>
                </tr>
              </tbody>
            </table>
            <table id="five">
              <table>
                <tbody>
                  <tr>
                    <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                  </tr>
                  <tr>
                    <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td rowspan="2">&nbsp;</td>
                  </tr>
                  <tr>
                    <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                  </tr>
                  <tr>
                    <td>&nbsp;</td><td colspan="2">&nbsp;</td><td>&nbsp;</td>
                  </tr>
                  <tr>
                    <td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td>
                  </tr>
                  <tr>
                    <td colspan="4">&nbsp;</td>
                  </tr>
                </tbody>
              </table>
            </table>
          </body>
        </html>
        "#;
        let mut doc = Readability::new(html_str);
        doc.mark_data_tables();
        let target = doc.root_node.select_first("table#one").unwrap();
        let target_attr = target.attributes.borrow();
        assert_eq!(Some("false"), target_attr.get("readability-data-table"));

        let target = doc.root_node.select_first("table#two").unwrap();
        let target_attr = target.attributes.borrow();
        assert_eq!(Some("false"), target_attr.get("readability-data-table"));

        let target = doc.root_node.select_first("table#three").unwrap();
        let target_attr = target.attributes.borrow();
        assert_eq!(Some("true"), target_attr.get("readability-data-table"));

        let target = doc.root_node.select_first("table#four").unwrap();
        let target_atrr = target.attributes.borrow();
        assert_eq!(Some("true"), target_atrr.get("readability-data-table"));

        let target = doc.root_node.select_first("table#five").unwrap();
        let target_atrr = target.attributes.borrow();
        assert_eq!(Some("false"), target_atrr.get("readability-data-table"));
    }

    #[test]
    fn test_fix_lazy_images() {}
}
