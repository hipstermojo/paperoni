use std::collections::{BTreeMap, HashMap};

use crate::extractor::MetaAttr;

use html5ever::{LocalName, Namespace, QualName};
use kuchiki::{
    iter::{Descendants, Elements, Select},
    traits::*,
    NodeData, NodeRef,
};

const SHARE_ELEMENT_THRESHOLD: usize = 500;
const READABILITY_SCORE: &'static str = "readability-score";
const HTML_NS: &'static str = "http://www.w3.org/1999/xhtml";
// TODO: Change to HashSet
const PHRASING_ELEMS: [&str; 39] = [
    "abbr", "audio", "b", "bdo", "br", "button", "cite", "code", "data", "datalist", "dfn", "em",
    "embed", "i", "img", "input", "kbd", "label", "mark", "math", "meter", "noscript", "object",
    "output", "progress", "q", "ruby", "samp", "script", "select", "small", "span", "strong",
    "sub", "sup", "textarea", "time", "var", "wbr",
];
// TODO: Change to HashSet
const DEFAULT_TAGS_TO_SCORE: [&str; 9] =
    ["section", "h2", "h3", "h4", "h5", "h6", "p", "td", "pre"];
// TODO: Change to HashSet
const ALTER_TO_DIV_EXCEPTIONS: [&str; 4] = ["div", "article", "section", "p"];
const PRESENTATIONAL_ATTRIBUTES: [&str; 12] = [
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

const DATA_TABLE_DESCENDANTS: [&str; 5] = ["col", "colgroup", "tfoot", "thead", "th"];
// TODO: Change to HashSet
const DEPRECATED_SIZE_ATTRIBUTE_ELEMS: [&str; 5] = ["table", "th", "td", "hr", "pre"];

mod regexes;

pub struct Readability {
    root_node: NodeRef,
    byline: Option<String>,
    article_title: String,
    pub article_node: Option<NodeRef>,
    article_dir: Option<String>,
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
            article_node: None,
            article_dir: None,
        }
    }
    pub fn parse(&mut self) {
        self.unwrap_no_script_tags();
        self.remove_scripts();
        self.prep_document();
        // TODO: Add implementation for get_article_metadata
        self.grab_article();
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
            let mut nodes = imgs.filter(|img_node_ref| {
                let img_attrs = img_node_ref.attributes.borrow();
                !img_attrs.map.iter().any(|(name, attr)| {
                    &name.local == "src"
                        || &name.local == "srcset"
                        || &name.local == "data-src"
                        || &name.local == "data-srcset"
                        || regexes::is_match_img_ext(&attr.value)
                })
            });
            let mut node_ref = nodes.next();
            while let Some(img_ref) = node_ref {
                node_ref = nodes.next();
                img_ref.as_node().detach();
            }
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
                                    || regexes::is_match_img_ext(&val.value))
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
                        // WARN: This assumes `next_element` returns an element node!!
                        let inner_node_child =
                            Self::next_element(inner_node_ref.first_child(), true);
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
            Ok(mut script_elems) => {
                let mut next_script = script_elems.next();
                while let Some(next_script_ref) = next_script {
                    next_script = script_elems.next();
                    next_script_ref.as_node().detach();
                }
            }
            Err(_) => (),
        }
        match self.root_node.select("noscript") {
            Ok(mut noscript_elems) => {
                let mut next_noscript = noscript_elems.next();
                while let Some(noscript_ref) = next_noscript {
                    next_noscript = noscript_elems.next();
                    noscript_ref.as_node().detach();
                }
            }
            Err(_) => (),
        }
    }

    /// Prepare the HTML document for readability to scrape it. This includes things like stripping
    /// CSS, and handling terrible markup.
    fn prep_document(&mut self) {
        match self.root_node.select("style") {
            Ok(mut style_elems) => {
                let mut style_elem = style_elems.next();
                while let Some(style_ref) = style_elem {
                    style_elem = style_elems.next();
                    style_ref.as_node().detach();
                }
            }
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
            // The uses of `next_element` here are safe as it explicitly ensures the next element is an element node
            while let Some(br_tag) = br_tags.next() {
                let mut next = Self::next_element(br_tag.as_node().next_sibling(), false);
                let mut replaced = false;
                while let Some(next_elem) = next {
                    if next_elem.as_element().is_some()
                        && &next_elem.as_element().as_ref().unwrap().name.local == "br"
                    {
                        replaced = true;
                        let br_sibling = next_elem.next_sibling();
                        next = Self::next_element(br_sibling, false);
                        next_elem.detach();
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
    fn set_node_tag(node_ref: &NodeRef, name: &str) -> NodeRef {
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
                let new_node = node_ref.previous_sibling().unwrap();
                node_ref.detach();
                return new_node;
            }
            None => (),
        }
        node_ref.clone()
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
    /// The must_be_element argument ensure the next element is actually an element node.
    /// This is likely to factored out into a new function.
    fn next_element(node_ref: Option<NodeRef>, must_be_element: bool) -> Option<NodeRef> {
        // TODO: Could probably be refactored to use the elements method
        let mut node_ref = node_ref;
        while node_ref.is_some() {
            match node_ref.as_ref().unwrap().data() {
                NodeData::Element(_) => break,
                _ => {
                    if node_ref.as_ref().unwrap().text_contents().trim().is_empty() {
                        node_ref = node_ref.as_ref().unwrap().next_sibling();
                    } else if must_be_element
                        && !node_ref.as_ref().unwrap().text_contents().trim().is_empty()
                    {
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
                let is_byline = (if rel_attr.is_some() {
                    rel_attr.unwrap() == "author"
                } else if itemprop_attr.is_some() {
                    itemprop_attr.unwrap().contains("author")
                } else {
                    regexes::is_match_byline(match_string)
                }) && Self::is_valid_byline(&node_ref.text_contents());
                if is_byline {
                    self.byline = Some(node_ref.text_contents().trim().to_owned());
                }
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
        // WARN: The uses of `next_element` here assume it returns an element node.
        let has_elem_children = node_ref.children().elements().count();
        if !ignore_self_and_kids && has_elem_children > 0 {
            Self::next_element(node_ref.first_child(), true)
        } else if let Some(next_sibling) = Self::next_element(node_ref.next_sibling(), true) {
            Some(next_sibling)
        } else {
            // Keep walking up the node hierarchy until a parent with element siblings is found
            let mut node = node_ref.parent();
            while let Some(parent) = node {
                if let Some(next_sibling) = Self::next_element(parent.next_sibling(), true) {
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
                && regexes::is_match_has_content(&node.text_contents().trim_end())
        })
    }

    fn get_inner_text(node_ref: &NodeRef, normalize_spaces: Option<bool>) -> String {
        let will_normalize = normalize_spaces.unwrap_or(true);
        let text = node_ref.text_contents();
        let text = text.trim();
        if will_normalize {
            return regexes::NORMALIZE_REGEX.replace_all(&text, " ").to_string();
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
        let node_elem = node_ref.as_element().unwrap();
        let node_attrs = node_elem.attributes.borrow();
        if let Some(id) = node_attrs.get("id") {
            if !id.trim().is_empty() {
                weight = if regexes::is_match_positive(id) {
                    weight + 25
                } else if regexes::is_match_negative(id) {
                    weight - 25
                } else {
                    weight
                }
            }
        }
        if let Some(class) = node_attrs.get("class") {
            if !class.trim().is_empty() {
                weight = if regexes::is_match_positive(class) {
                    weight + 25
                } else if regexes::is_match_negative(class) {
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
            let mut score = 0.0;
            // This must be computed first because it borrows the NodeRef which
            // should not also be mutably borrowed
            score += Self::get_class_weight(node_ref) as f32;
            let mut elem_attrs = element.attributes.borrow_mut();
            elem_attrs.insert(READABILITY_SCORE, score.to_string());
            let readability = elem_attrs.get_mut(READABILITY_SCORE);
            match &*element.name.local {
                "div" => score += 5.0,
                "pre" | "td" | "blockquote" => score += 3.0,
                "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => score -= 3.0,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => score -= 5.0,
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

                if DATA_TABLE_DESCENDANTS
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
        let nodes = node_ref.select("img, picture, figure").unwrap();
        for node in nodes {
            let mut node_attr = node.attributes.borrow_mut();
            if let Some(src) = node_attr.get("src") {
                let src_captures = regexes::B64_DATA_URL_REGEX.captures(src);
                if src_captures.is_some() {
                    let svg_capture = src_captures.unwrap().get(1);
                    if svg_capture.is_some() && svg_capture.unwrap().as_str() == "image/svg+xml" {
                        continue;
                    }

                    let src_could_be_removed = node_attr
                        .map
                        .iter()
                        .filter(|(name, _)| &name.local != "src")
                        .filter(|(_, val)| regexes::is_match_img_ext(&val.value))
                        .count()
                        > 0;

                    if src_could_be_removed {
                        let b64_start = regexes::BASE64_REGEX.find(src).unwrap().start();
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
            if (src.is_some() || srcset.is_some())
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
                    if regexes::is_match_srcset(&val.value) {
                        copy_to = "srcset";
                    } else if regexes::is_match_src_regex(&val.value) {
                        copy_to = "src";
                    }
                    if copy_to.len() > 0 {
                        let new_val = val.value.clone();
                        let tag_name = &node.name.local;
                        if tag_name == "img" || tag_name == "picture" {
                            node_attr.insert(copy_to, new_val);
                        } else if tag_name == "figure" {
                            let node_ref = node.as_node();
                            let img_picture_nodes = node_ref.select("img, picture").unwrap();
                            if img_picture_nodes.count() > 0 {
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
                                    img_attr.insert(copy_to, new_val);
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
        let is_data_table = |node_ref: &NodeRef| {
            let node_elem = node_ref.as_element().unwrap();
            let attrs = node_elem.attributes.borrow();
            attrs.get("readability-data-table") == Some("true")
        };
        let get_char_count = |node_ref: &NodeRef| node_ref.text_contents().matches(",").count();

        let mut nodes = node_ref
            .descendants()
            .select(tag_name)
            .unwrap()
            // Do not remove data tables
            .filter(|node_data_ref| {
                !(&node_data_ref.name.local == "table" && is_data_table(node_data_ref.as_node()))
            })
            // Do not remove if it is a child of a data table
            .filter(|node_data_ref| {
                !Self::has_ancestor_tag(
                    node_data_ref.as_node(),
                    tag_name,
                    Some(-1),
                    Some(is_data_table),
                )
            });
        let mut next_node = nodes.next();
        while let Some(node_data_ref) = next_node {
            next_node = nodes.next();
            let node = node_data_ref.as_node();
            let weight = Self::get_class_weight(node);
            // Remove all elements with negative class weights
            if weight < 0 {
                node.detach();
                continue;
            }

            if get_char_count(node) >= 10 {
                continue;
            }
            let mut embeds = node_data_ref
                .as_node()
                .select("object, embed, iframe")
                .unwrap();
            let can_skip_embed = embeds.any(|node_data_ref| {
                &node_data_ref.name.local == "object" || {
                    let attrs = node_data_ref.attributes.borrow();

                    attrs
                        .map
                        .iter()
                        .any(|(_, val)| regexes::is_match_videos(&val.value))
                }
            });
            if can_skip_embed {
                continue;
            }

            let p_nodes = node_data_ref.as_node().select("p").unwrap().count();
            let img_nodes = node_data_ref.as_node().select("img").unwrap().count();
            let li_nodes = node_data_ref.as_node().select("li").unwrap().count() as i32 - 100;
            let input_nodes = node_data_ref.as_node().select("input").unwrap().count();

            let p = p_nodes as f32;
            let img = img_nodes as f32;

            let embed_count = node.select("object, embed, iframe").unwrap().count();
            let link_density = Self::get_link_density(node);
            let content_length = Self::get_inner_text(node, None).len();
            let has_figure_ancestor = Self::has_ancestor_tag(node, "figure", None, None);
            let have_to_remove = (img_nodes > 1 && p / img < 0.5 && !has_figure_ancestor)
                || (!is_list && li_nodes > p_nodes as i32)
                || (input_nodes > (p_nodes / 3))
                || (!is_list
                    && content_length < 25
                    && (img_nodes == 0 || img_nodes > 2)
                    && !has_figure_ancestor)
                || (!is_list && weight < 25 && link_density > 0.2)
                || (weight >= 25 && link_density > 0.5)
                || ((embed_count == 1 && content_length < 75) || embed_count > 1);
            if have_to_remove {
                node.detach();
            }
        }
    }

    /// Clean a node of all elements of type "tag". (Unless it's a YouTube or Vimeo video)
    fn clean(node_ref: &mut NodeRef, tag_name: &str) {
        // Can be changed to a HashSet
        let is_embed = vec!["object", "embed", "iframe"].contains(&tag_name);
        let mut nodes = node_ref
            .descendants()
            .select(tag_name)
            .unwrap()
            .filter(|node_data_ref| {
                !is_embed
                    || {
                        let attrs = node_data_ref.attributes.borrow();
                        !attrs
                            .map
                            .iter()
                            .any(|(_, val)| regexes::is_match_videos(&val.value))
                    }
                    || &node_data_ref.name.local == "object" // This currently does not check the innerHTML.
            });
        let mut node = nodes.next();
        while let Some(node_data_ref) = node {
            node = nodes.next();
            node_data_ref.as_node().detach()
        }
    }

    /// Clean out spurious headers from an Element. Checks things like classnames and link density.
    fn clean_headers(node_ref: &mut NodeRef) {
        let mut nodes = node_ref
            .descendants()
            .select("h1, h2")
            .unwrap()
            .filter(|node_data_ref| Self::get_class_weight(node_data_ref.as_node()) < 0);
        let mut node = nodes.next();

        while let Some(node_data_ref) = node {
            node = nodes.next();
            node_data_ref.as_node().detach();
        }
    }

    /// Remove the style attribute on every element and descendants.
    fn clean_styles(node_ref: &mut NodeRef) {
        node_ref
            .inclusive_descendants()
            .elements()
            .filter(|node| &node.name.local != "svg")
            .for_each(|node_data_ref| {
                let mut attrs = node_data_ref.attributes.borrow_mut();
                PRESENTATIONAL_ATTRIBUTES.iter().for_each(|pres_attr| {
                    attrs.remove(*pres_attr);
                });
                if DEPRECATED_SIZE_ATTRIBUTE_ELEMS.contains(&node_data_ref.name.local.as_ref()) {
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
        self.mark_data_tables();
        Self::fix_lazy_images(node_ref);
        Self::clean_conditionally(node_ref, "form");
        Self::clean_conditionally(node_ref, "fieldset");
        Self::clean(node_ref, "object");
        Self::clean(node_ref, "embed");
        Self::clean(node_ref, "h1");
        Self::clean(node_ref, "footer");
        Self::clean(node_ref, "link");
        Self::clean(node_ref, "aside");

        node_ref.children().for_each(|mut node| {
            Self::clean_matched_nodes(&mut node, |node: &NodeRef, match_string| {
                regexes::is_match_share_elems(match_string)
                    && node.text_contents().len() < SHARE_ELEMENT_THRESHOLD
            });
        });

        let h2_nodes = node_ref.select("h2").unwrap().take(2).collect::<Vec<_>>();
        if h2_nodes.len() == 1 {
            let h2_node = h2_nodes[0].as_node();
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

        let mut p_nodes = node_ref.select("p").unwrap().filter(|node_data_ref| {
            let p_node = node_data_ref.as_node();
            let img_count = p_node.select("img").unwrap().count();
            let embed_count = p_node.select("embed").unwrap().count();
            let object_count = p_node.select("object").unwrap().count();
            let iframe_count = p_node.select("iframe").unwrap().count();
            let total = img_count + embed_count + object_count + iframe_count;
            total == 0 && Self::get_inner_text(node_data_ref.as_node(), Some(false)).is_empty()
        });
        let mut p_node = p_nodes.next();
        while let Some(p_node_ref) = p_node {
            p_node = p_nodes.next();
            p_node_ref.as_node().detach();
        }

        let mut br_nodes = node_ref.select("br").unwrap().filter(|node_data_ref| {
            let br_node = node_data_ref.as_node();
            // WARN: This assumes `next_element` returns an element node.
            let next_node = Self::next_element(br_node.next_sibling(), true);
            next_node.is_some() && &next_node.unwrap().as_element().unwrap().name.local == "p"
        });
        let mut br_node = br_nodes.next();
        while let Some(br_node_ref) = br_node {
            br_node = br_nodes.next();
            br_node_ref.as_node().detach();
        }

        let mut table_nodes = node_ref.select("table").unwrap();
        let mut table_node = table_nodes.next();
        while let Some(table_node_ref) = table_node {
            table_node = table_nodes.next();
            let table_node = table_node_ref.as_node();
            // WARN: This assumes `next_element` returns an element node.
            let table_child = Self::next_element(table_node.first_child(), true);
            let tbody = if Self::has_single_tag_inside_element(&table_node, "tbody") {
                table_child.as_ref().unwrap()
            } else {
                table_node
            };

            // WARN: This block assumes `next_element` returns an element node
            if Self::has_single_tag_inside_element(&tbody, "tr") {
                let row = Self::next_element(tbody.first_child(), true).unwrap();
                if Self::has_single_tag_inside_element(&row, "td") {
                    let mut cell = Self::next_element(row.first_child(), true).unwrap();
                    let tag = if cell
                        .children()
                        .all(|cell_child| Self::is_phrasing_content(&cell_child))
                    {
                        "p"
                    } else {
                        "div"
                    };
                    cell = Self::set_node_tag(&cell, tag);
                    if let Some(parent) = table_node.parent() {
                        parent.append(cell);
                        table_node.detach();
                    }
                }
            }
        }
    }

    /// Using a variety of metrics (content score, classname, element types), find the content that is most likely to be the stuff
    /// a user wants to read. Then return it wrapped up in a div.
    fn grab_article(&mut self) {
        println!("Grabbing article");
        // var doc = this._doc;
        // var isPaging = (page !== null ? true: false);
        // page = page ? page : this._doc.body;
        let page = self.root_node.select_first("body");
        if page.is_err() {
            // TODO:Have error logging for this
            println!("Document has no <body>");
            return;
        }
        let page = page.unwrap();

        // // We can't grab an article if we don't have a page!
        // if (!page) {
        //   this.log("No body found in document. Abort.");
        //   return null;
        // }

        // var pageCacheHtml = page.innerHTML;

        loop {
            //   var stripUnlikelyCandidates = this._flagIsActive(this.FLAG_STRIP_UNLIKELYS);
            // TODO: Add flag for checking this
            let strip_unlikely_candidates = true;

            //   // First, node prepping. Trash nodes that look cruddy (like ones with the
            //   // class name "comment", etc), and turn divs into P tags where they have been
            //   // used inappropriately (as in, where they contain no other block level elements.)
            let mut elements_to_score: Vec<NodeRef> = Vec::new();
            let mut node = Some(
                self.root_node
                    .select_first("html")
                    .unwrap()
                    .as_node()
                    .clone(),
            );

            while let Some(node_ref) = node {
                let node_elem = node_ref.as_element().unwrap();
                let node_name: &str = node_elem.name.local.as_ref();
                let match_string = {
                    let node_attrs = node_elem.attributes.borrow();
                    node_attrs.get("class").unwrap_or("").to_string()
                        + " "
                        + node_attrs.get("id").unwrap_or("")
                };
                if !Self::is_probably_visible(&node_ref) {
                    node = Self::remove_and_get_next(node_ref);
                    continue;
                }

                if self.check_byline(&node_ref, &match_string) {
                    node = Self::remove_and_get_next(node_ref);
                    continue;
                }

                if strip_unlikely_candidates {
                    if regexes::is_match_unlikely(&match_string)
                        && !regexes::is_match_ok_maybe(&match_string)
                        && !Self::has_ancestor_tag(&node_ref, "table", None, None)
                        && node_name != "body"
                        && node_name != "a"
                    {
                        node = Self::remove_and_get_next(node_ref);
                        continue;
                    }

                    let is_complementary = {
                        let node_attrs = node_elem.attributes.borrow();
                        node_attrs.get("role") == Some("complementary")
                    };
                    if is_complementary {
                        node = Self::remove_and_get_next(node_ref);
                        continue;
                    }
                }

                match node_name {
                    "div" | "section" | "header" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if Self::is_element_without_content(&node_ref) {
                            node = Self::remove_and_get_next(node_ref);
                            continue;
                        }
                    }
                    _ => (),
                }
                if !DEFAULT_TAGS_TO_SCORE.contains(&node_name) {
                    elements_to_score.push(node_ref.clone());
                }
                if node_name == "div" {
                    let mut p: Option<NodeRef> = None;
                    let mut child_node = node_ref.first_child();
                    while let Some(child_node_ref) = child_node {
                        let next_sibling = child_node_ref.next_sibling();
                        if Self::is_phrasing_content(&child_node_ref) {
                            if let Some(ref p_node) = p {
                                p_node.append(child_node_ref);
                            } else if !Self::is_whitespace(&child_node_ref) {
                                let new_p_node = NodeRef::new_element(
                                    QualName::new(
                                        None,
                                        Namespace::from(HTML_NS),
                                        LocalName::from("p"),
                                    ),
                                    BTreeMap::new(),
                                );
                                child_node_ref.insert_before(new_p_node);
                                p = child_node_ref.previous_sibling();
                                // Append will implicitly detach the child_node_ref
                                p.as_mut().unwrap().append(child_node_ref);
                            }
                        } else if let Some(ref p_node) = p {
                            while let Some(last_child) = p_node.last_child() {
                                if Self::is_whitespace(&last_child) {
                                    last_child.detach();
                                } else {
                                    break;
                                }
                            }
                            p = None;
                        }
                        child_node = next_sibling;
                    }
                    if Self::has_single_tag_inside_element(&node_ref, "p")
                        && Self::get_link_density(&node_ref) < 0.25
                    {
                        // WARN: This assumes `next_element` returns an element node.
                        let new_node = Self::next_element(node_ref.first_child(), true).unwrap();
                        elements_to_score.push(new_node.clone());
                        node_ref.insert_before(new_node);
                        let new_node = node_ref.previous_sibling();
                        node_ref.detach();
                        node = new_node;
                        elements_to_score.push(node.clone().unwrap());
                    } else if !Self::has_child_block_element(&node_ref) {
                        node = Some(Self::set_node_tag(&node_ref, "p"));
                        elements_to_score.push(node.clone().unwrap());
                    }
                }
                node = Self::get_next_node(&node_ref, false);
            }

            let mut candidates: Vec<NodeRef> = Vec::new();
            elements_to_score
                .iter()
                .filter(|node_ref| {
                    let parent = node_ref.parent();
                    parent.is_some() && parent.unwrap().as_element().is_some()
                })
                .map(|node_ref| (node_ref, Self::get_inner_text(&node_ref, None)))
                .filter(|(_, inner_text)| inner_text.len() >= 25)
                .map(|(node_ref, inner_text)| {
                    (inner_text, Self::get_node_ancestors(&node_ref, Some(3)))
                })
                .filter(|(_, ancestors)| ancestors.len() != 0)
                .for_each(|(inner_text, ancestors)| {
                    let mut content_score = 0;
                    content_score += 1;
                    content_score += inner_text.split(",").count();
                    content_score += (3).min(inner_text.len() / 100);
                    ancestors
                        .into_iter()
                        .enumerate()
                        .filter(|(_, node)| {
                            node.parent().is_some() && node.parent().unwrap().as_element().is_some()
                        })
                        .for_each(|(level, mut ancestor)| {
                            let has_readability = {
                                let ancestor_attrs =
                                    ancestor.as_element().unwrap().attributes.borrow();
                                ancestor_attrs.contains(READABILITY_SCORE)
                            };
                            if !has_readability {
                                Self::initialize_node(&mut ancestor);
                                candidates.push(ancestor.clone());
                            }

                            let score_divider = if level == 0 {
                                1.0
                            } else if level == 1 {
                                2.0
                            } else {
                                level as f32 * 3.0
                            };
                            let mut ancestor_attrs =
                                ancestor.as_element().unwrap().attributes.borrow_mut();
                            if let Some(readability_score) =
                                ancestor_attrs.get_mut(READABILITY_SCORE)
                            {
                                *readability_score = (readability_score.parse::<f32>().unwrap()
                                    + (content_score as f32 / score_divider))
                                    .to_string();
                            }
                        });
                });

            let mut top_candidates: Vec<NodeRef> = Vec::new();
            for candidate in candidates {
                let mut candidate_score = 0.0;
                {
                    let mut candidate_attr =
                        candidate.as_element().unwrap().attributes.borrow_mut();
                    if let Some(readability_score) = candidate_attr.get_mut(READABILITY_SCORE) {
                        candidate_score = readability_score.parse::<f32>().unwrap()
                            * (1.0 - Self::get_link_density(&candidate));
                        *readability_score = candidate_score.to_string();
                    }
                }
                let nb_top_candidates = 5;
                for i in 0..nb_top_candidates {
                    let top_candidate = top_candidates.get(i);
                    let top_candidate_score = top_candidate
                        .as_ref()
                        .map(|node_ref| node_ref.as_element().unwrap().attributes.borrow())
                        .map(|attrs| {
                            attrs
                                .get(READABILITY_SCORE)
                                .unwrap_or("0")
                                .parse::<f32>()
                                .unwrap()
                        });
                    if top_candidate.is_none() || candidate_score > top_candidate_score.unwrap() {
                        top_candidates.splice(i..i, vec![candidate].into_iter());
                        if top_candidates.len() > nb_top_candidates {
                            top_candidates.pop();
                        }
                        break;
                    }
                }
            }

            let possible_top_candidate = top_candidates.get(0);
            let mut top_candidate;
            let mut needed_to_create_top_candidate = false;
            let mut parent_of_top_candidate: NodeRef;

            if possible_top_candidate.is_none()
                || possible_top_candidate
                    .map(|node| &node.as_element().unwrap().name.local)
                    .as_ref()
                    .unwrap()
                    == &"body"
            {
                top_candidate = NodeRef::new_element(
                    QualName::new(None, Namespace::from(HTML_NS), LocalName::from("div")),
                    BTreeMap::new(),
                );
                needed_to_create_top_candidate = true;
                page.as_node().children().for_each(|child_node| {
                    top_candidate.append(child_node);
                });
                page.as_node().append(top_candidate.clone());
                Self::initialize_node(&mut top_candidate);
            } else {
                let alternative_candidate_ancestors: Vec<Vec<NodeRef>>;
                top_candidate = top_candidates.get(0).unwrap().clone();
                let top_candidate_score = {
                    let top_candidate_node_attrs =
                        top_candidate.as_element().unwrap().attributes.borrow();
                    top_candidate_node_attrs
                        .get(READABILITY_SCORE)
                        .unwrap()
                        .parse::<f32>()
                        .unwrap()
                };

                alternative_candidate_ancestors = top_candidates
                    .iter()
                    .skip(1)
                    .filter(|top_candidate_node| {
                        let candidate_node_score = {
                            let top_candidate_node_attrs =
                                top_candidate_node.as_element().unwrap().attributes.borrow();
                            top_candidate_node_attrs
                                .get(READABILITY_SCORE)
                                .unwrap()
                                .parse::<f32>()
                                .unwrap()
                        };
                        (candidate_node_score / top_candidate_score) >= 0.75
                    })
                    .map(|node| Self::get_node_ancestors(&node, None))
                    .collect();

                let minimum_top_candidates = 3;
                if alternative_candidate_ancestors.len() >= minimum_top_candidates {
                    parent_of_top_candidate = top_candidate.parent().unwrap();
                    while &parent_of_top_candidate.as_element().unwrap().name.local != "body" {
                        let mut lists_containing_this_ancestor = alternative_candidate_ancestors
                            .iter()
                            .filter(|node_vec| node_vec.contains(&parent_of_top_candidate))
                            .count();
                        lists_containing_this_ancestor =
                            lists_containing_this_ancestor.min(minimum_top_candidates);
                        if lists_containing_this_ancestor >= minimum_top_candidates {
                            top_candidate = parent_of_top_candidate;
                            break;
                        }
                        parent_of_top_candidate = parent_of_top_candidate.parent().unwrap();
                    }
                }

                let top_candidate_readability = {
                    let top_candidate_attrs =
                        top_candidate.as_element().unwrap().attributes.borrow();
                    top_candidate_attrs
                        .get(READABILITY_SCORE)
                        .map(|x| x.to_owned())
                };

                if top_candidate_readability.is_none() {
                    Self::initialize_node(&mut top_candidate);
                }
                parent_of_top_candidate = top_candidate.parent().unwrap();

                let mut last_score = {
                    let top_candidate_node_attrs =
                        top_candidate.as_element().unwrap().attributes.borrow();
                    top_candidate_node_attrs
                        .get(READABILITY_SCORE)
                        .unwrap()
                        .parse::<f32>()
                        .unwrap()
                };
                let score_threshold = last_score / 3.0;
                while parent_of_top_candidate
                    .as_element()
                    .map(|elem| elem.name.local.as_ref())
                    .unwrap()
                    != "body"
                {
                    let parent_readability = {
                        let parent_attrs = parent_of_top_candidate
                            .as_element()
                            .unwrap()
                            .attributes
                            .borrow();
                        parent_attrs
                            .get(READABILITY_SCORE)
                            .map(|score| score.parse::<f32>().unwrap())
                    };
                    if parent_readability.is_none() {
                        parent_of_top_candidate = parent_of_top_candidate.parent().unwrap();
                        continue;
                    }
                    if parent_readability.as_ref().unwrap() < &score_threshold {
                        break;
                    }
                    if parent_readability.as_ref().unwrap() > &last_score {
                        top_candidate = parent_of_top_candidate;
                        break;
                    }
                    last_score = parent_readability.unwrap();
                    parent_of_top_candidate = parent_of_top_candidate.parent().unwrap();
                }

                parent_of_top_candidate = top_candidate.parent().unwrap();
                while &parent_of_top_candidate.as_element().unwrap().name.local != "body"
                    && parent_of_top_candidate.children().count() == 1
                {
                    top_candidate = parent_of_top_candidate;
                    parent_of_top_candidate = top_candidate.parent().unwrap();
                }
                let top_candidate_readability = {
                    let top_candidate_attrs =
                        top_candidate.as_element().unwrap().attributes.borrow();
                    top_candidate_attrs
                        .get(READABILITY_SCORE)
                        .map(|score| score.to_string())
                };
                if top_candidate_readability.is_none() {
                    Self::initialize_node(&mut top_candidate);
                }
            }
            let mut article_content = NodeRef::new_element(
                QualName::new(None, Namespace::from(HTML_NS), LocalName::from("div")),
                BTreeMap::new(),
            );
            let top_candidate_score = {
                let top_candidate_attrs = top_candidate.as_element().unwrap().attributes.borrow();
                top_candidate_attrs
                    .get(READABILITY_SCORE)
                    .map(|score| score.parse::<f32>().unwrap())
                    .unwrap()
            };

            let sibling_score_threshold = (10.0_f32).max(top_candidate_score * 0.2);
            parent_of_top_candidate = top_candidate.parent().unwrap();
            let siblings = parent_of_top_candidate.children();

            let (top_candidate_class, top_candidate_score) = {
                let top_candidate_attrs = top_candidate.as_element().unwrap().attributes.borrow();
                let class = top_candidate_attrs
                    .get("class")
                    .map(|class| class.to_string())
                    .unwrap_or("".to_string());
                let score = top_candidate_attrs
                    .get(READABILITY_SCORE)
                    .map(|score| score.parse::<f32>().unwrap())
                    .unwrap();
                (class, score)
            };
            for sibling in siblings {
                let mut append = false;
                if sibling == top_candidate {
                    append = true;
                } else {
                    let mut content_bonus = 0.0;
                    let sibling_attrs = sibling.as_element().unwrap().attributes.borrow();

                    let sibling_class = sibling_attrs
                        .get("class")
                        .map(|class| class.to_string())
                        .unwrap_or("".to_string());
                    let sibling_score = sibling_attrs
                        .get(READABILITY_SCORE)
                        .map(|score| score.parse::<f32>().unwrap());

                    if sibling_class == top_candidate_class && !top_candidate_class.is_empty() {
                        content_bonus += top_candidate_score * 0.2;
                    }

                    if sibling_score.is_some()
                        && (sibling_score.unwrap() + content_bonus) >= sibling_score_threshold
                    {
                        append = true;
                    } else if sibling.as_element().map(|elem| elem.name.local.as_ref()) == Some("p")
                    {
                        let link_density = Self::get_link_density(&sibling);
                        let node_content = Self::get_inner_text(&sibling, None);
                        let node_length = node_content.len();
                        if node_length > 80 && link_density < 0.25 {
                            append = true;
                        } else if node_length < 80
                            && node_length > 0
                            && link_density == 0.0
                            && !regexes::is_match_node_content(&node_content)
                        {
                            append = true;
                        }
                    }
                }
                if append {
                    let new_article_child = if !ALTER_TO_DIV_EXCEPTIONS.contains(
                        &sibling
                            .as_element()
                            .map(|elem| elem.name.local.as_ref())
                            .unwrap(),
                    ) {
                        Self::set_node_tag(&sibling, "div")
                    } else {
                        sibling
                    };
                    article_content.append(new_article_child);
                }
            }
            self.prep_article(&mut article_content);
            if needed_to_create_top_candidate {
                let mut top_candidate_attrs =
                    top_candidate.as_element().unwrap().attributes.borrow_mut();
                top_candidate_attrs.insert("id", "readability-page-1".to_string());
                top_candidate_attrs.insert("class", "page".to_string());
            } else {
                let div = NodeRef::new_element(
                    QualName::new(None, Namespace::from(HTML_NS), LocalName::from("div")),
                    BTreeMap::new(),
                );
                {
                    let mut div_attrs = div.as_element().unwrap().attributes.borrow_mut();
                    div_attrs.insert("id", "readability-page-1".to_string());
                    div_attrs.insert("class", "page".to_string());
                }
                for child in article_content.children() {
                    div.append(child);
                }
                article_content.append(div);
            }

            let text_length = Self::get_inner_text(&article_content, Some(true)).len();
            let mut parse_successful = true;
            if text_length < 500 {
                // TODO Add flag checks
                parse_successful = false;
                println!("I haz a smol content. Plz run me again");
            }
            if parse_successful {
                let parent_ancestors = Self::get_node_ancestors(&parent_of_top_candidate, None);
                let ancestors = vec![
                    vec![parent_of_top_candidate, top_candidate],
                    parent_ancestors,
                ]
                .concat();
                ancestors.iter().any(|node| {
                    let node_elem = node.as_element();
                    if node_elem.is_none() {
                        return false;
                    }
                    let node_attrs = node_elem.unwrap().attributes.borrow();
                    if let Some(dir_attr) = node_attrs.get("dir") {
                        self.article_dir = Some(dir_attr.to_string());
                        return true;
                    }
                    false
                });
                self.article_node = Some(article_content);
                return;
            }
            // TODO: Remove this
            break;
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Readability, SizeInfo, HTML_NS, READABILITY_SCORE};
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

         This is standalone text
         <p> Some <span>more</span> text</p>"#;
        let doc = Readability::new(html_str);
        let p = doc.root_node.select_first("#a").unwrap();
        let p = p.as_node();
        let mut p_node_option: Option<NodeRef> = Some(p.clone());
        p_node_option = Readability::next_element(p_node_option, false);
        assert_eq!(Some(p.clone()), p_node_option);

        let p_node_option = p_node_option.unwrap();
        let p_node_option = p_node_option.as_element();
        let p_node_option_attr = p_node_option.unwrap().attributes.borrow();
        assert_eq!("a", p_node_option_attr.get("id").unwrap());

        let next = Readability::next_element(p.next_sibling(), false);

        let next = next.unwrap();
        let next_elem = next.as_element();
        let next_attr = next_elem.unwrap().attributes.borrow();
        assert_eq!("b", next_attr.get("id").unwrap());

        let next = Readability::next_element(next.next_sibling(), false);

        let next = next.unwrap();
        assert_eq!(true, next.as_text().is_some());
        assert_eq!("This is standalone text", next.text_contents().trim());

        let next = Readability::next_element(None, false);
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
        let new_node = Readability::set_node_tag(target.as_node(), "section");

        assert_eq!(children_count, doc.root_node.children().count());
        let target = doc.root_node.select_first("#target").unwrap();
        assert_eq!(&new_node, target.as_node());
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
        assert_eq!(Some("55"), node_attrs.get(READABILITY_SCORE));

        target = doc.root_node.select_first("h1.hidden").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-30"), node_attrs.get(READABILITY_SCORE));

        target = doc.root_node.select_first("p#story").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("25"), node_attrs.get(READABILITY_SCORE));

        target = doc.root_node.select_first("div#comments").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-20"), node_attrs.get(READABILITY_SCORE));

        target = doc.root_node.select_first("pre.comment").unwrap();
        let mut node = target.as_node().clone();
        Readability::initialize_node(&mut node);
        let node_attrs = node.as_element().unwrap().attributes.borrow();
        assert_eq!(Some("-22"), node_attrs.get(READABILITY_SCORE));
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
    fn test_fix_lazy_images() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <img id="svg-uri" alt="Basketball" src="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHhtbG5zOnhsaW5rPSJodHRwOi8vd3d3LnczLm9yZy8xOTk5L3hsaW5rIiB2ZXJzaW9uPSIxLjEiIGlkPSJMYXllcl8xIiB4PSIwcHgiIHk9IjBweCIgdmlld0JveD0iMCAwIDEwMCAxMDAiIGVuYWJsZS1iYWNrZ3JvdW5kPSJuZXcgMCAwIDEwMCAxMDAiIHhtbDpzcGFjZT0icHJlc2VydmUiIGhlaWdodD0iMTAwcHgiIHdpZHRoPSIxMDBweCI+CjxnPgoJPHBhdGggZD0iTTI4LjEsMzYuNmM0LjYsMS45LDEyLjIsMS42LDIwLjksMS4xYzguOS0wLjQsMTktMC45LDI4LjksMC45YzYuMywxLjIsMTEuOSwzLjEsMTYuOCw2Yy0xLjUtMTIuMi03LjktMjMuNy0xOC42LTMxLjMgICBjLTQuOS0wLjItOS45LDAuMy0xNC44LDEuNEM0Ny44LDE3LjksMzYuMiwyNS42LDI4LjEsMzYuNnoiLz4KCTxwYXRoIGQ9Ik03MC4zLDkuOEM1Ny41LDMuNCw0Mi44LDMuNiwzMC41LDkuNWMtMyw2LTguNCwxOS42LTUuMywyNC45YzguNi0xMS43LDIwLjktMTkuOCwzNS4yLTIzLjFDNjMuNywxMC41LDY3LDEwLDcwLjMsOS44eiIvPgoJPHBhdGggZD0iTTE2LjUsNTEuM2MwLjYtMS43LDEuMi0zLjQsMi01LjFjLTMuOC0zLjQtNy41LTctMTEtMTAuOGMtMi4xLDYuMS0yLjgsMTIuNS0yLjMsMTguN0M5LjYsNTEuMSwxMy40LDUwLjIsMTYuNSw1MS4zeiIvPgoJPHBhdGggZD0iTTksMzEuNmMzLjUsMy45LDcuMiw3LjYsMTEuMSwxMS4xYzAuOC0xLjYsMS43LTMuMSwyLjYtNC42YzAuMS0wLjIsMC4zLTAuNCwwLjQtMC42Yy0yLjktMy4zLTMuMS05LjItMC42LTE3LjYgICBjMC44LTIuNywxLjgtNS4zLDIuNy03LjRjLTUuMiwzLjQtOS44LDgtMTMuMywxMy43QzEwLjgsMjcuOSw5LjgsMjkuNyw5LDMxLjZ6Ii8+Cgk8cGF0aCBkPSJNMTUuNCw1NC43Yy0yLjYtMS02LjEsMC43LTkuNywzLjRjMS4yLDYuNiwzLjksMTMsOCwxOC41QzEzLDY5LjMsMTMuNSw2MS44LDE1LjQsNTQuN3oiLz4KCTxwYXRoIGQ9Ik0zOS44LDU3LjZDNTQuMyw2Ni43LDcwLDczLDg2LjUsNzYuNGMwLjYtMC44LDEuMS0xLjYsMS43LTIuNWM0LjgtNy43LDctMTYuMyw2LjgtMjQuOGMtMTMuOC05LjMtMzEuMy04LjQtNDUuOC03LjcgICBjLTkuNSwwLjUtMTcuOCwwLjktMjMuMi0xLjdjLTAuMSwwLjEtMC4yLDAuMy0wLjMsMC40Yy0xLDEuNy0yLDMuNC0yLjksNS4xQzI4LjIsNDkuNywzMy44LDUzLjksMzkuOCw1Ny42eiIvPgoJPHBhdGggZD0iTTI2LjIsODguMmMzLjMsMiw2LjcsMy42LDEwLjIsNC43Yy0zLjUtNi4yLTYuMy0xMi42LTguOC0xOC41Yy0zLjEtNy4yLTUuOC0xMy41LTktMTcuMmMtMS45LDgtMiwxNi40LTAuMywyNC43ICAgQzIwLjYsODQuMiwyMy4yLDg2LjMsMjYuMiw4OC4yeiIvPgoJPHBhdGggZD0iTTMwLjksNzNjMi45LDYuOCw2LjEsMTQuNCwxMC41LDIxLjJjMTUuNiwzLDMyLTIuMyw0Mi42LTE0LjZDNjcuNyw3Niw1Mi4yLDY5LjYsMzcuOSw2MC43QzMyLDU3LDI2LjUsNTMsMjEuMyw0OC42ICAgYy0wLjYsMS41LTEuMiwzLTEuNyw0LjZDMjQuMSw1Ny4xLDI3LjMsNjQuNSwzMC45LDczeiIvPgo8L2c+Cjwvc3ZnPg==" />
                <img id="normal-src" src="./foo.jpg">
                <img id="gif-uri" src="data:image/gif;base64,R0lGODlhEAAQAMQAAORHHOVSKudfOulrSOp3WOyDZu6QdvCchPGolfO0o/XBs/fNwfjZ0frl3/zy7////wAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACH5BAkAABAALAAAAAAQABAAAAVVICSOZGlCQAosJ6mu7fiyZeKqNKToQGDsM8hBADgUXoGAiqhSvp5QAnQKGIgUhwFUYLCVDFCrKUE1lBavAViFIDlTImbKC5Gm2hB0SlBCBMQiB0UjIQA7" alt="star" width="16" height="16">
                <img id="gif-uri-remove-src" data-src="./not-real-gif.png" src="data:image/gif;base64,R0lGODlhEAAQAMQAAORHHOVSKudfOulrSOp3WOyDZu6QdvCchPGolfO0o/" alt="star" width="16" height="16">
                <img id="lazy-loaded" class="lazy" src="placeholder.jpg" data-src="./720x640.jpg">
                <picture>
                    <source media="(min-width:650px)" srcset="img_pink_flowers.jpg">
                    <source media="(min-width:465px)" srcset="img_white_flower.jpg">
                    <img src="img_orange_flowers.jpg" alt="Flowers" style="width:auto;">
                </picture>
            </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let svg_uri = doc.root_node.select_first("#svg-uri").unwrap();
        let normal_src = doc.root_node.select_first("#normal-src").unwrap();
        let gif_uri = doc.root_node.select_first("#gif-uri").unwrap();
        let picture = doc.root_node.select_first("picture").unwrap();
        Readability::fix_lazy_images(&mut doc.root_node.clone());
        assert_eq!(svg_uri, doc.root_node.select_first("#svg-uri").unwrap());
        assert_eq!(
            normal_src,
            doc.root_node.select_first("#normal-src").unwrap()
        );
        assert_eq!(gif_uri, doc.root_node.select_first("#gif-uri").unwrap());
        assert_eq!(picture, doc.root_node.select_first("picture").unwrap());

        let gif_uri_remove_src = doc.root_node.select_first("#gif-uri-remove-src").unwrap();
        let gif_uri_remove_src_attrs = gif_uri_remove_src.attributes.borrow();
        assert_eq!(
            gif_uri_remove_src_attrs.get("data-src"),
            gif_uri_remove_src_attrs.get("src")
        );
        let lazy_loaded = doc.root_node.select_first("#lazy-loaded").unwrap();
        let lazy_loaded_attrs = lazy_loaded.attributes.borrow();
        assert_eq!(
            lazy_loaded_attrs.get("data-src"),
            lazy_loaded_attrs.get("src")
        );
    }

    #[test]
    fn test_clean_conditionally() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <table id="data-table">
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
                <table width="100%" border="0" id="display-table">
                    <tr valign="top">
                        <td width="20%">Left</td>
                        <td height="200" width="60%">Main</td>
                        <td width="20%">Right</td>
                    </tr>
                </table>
                <table width="100%" border="0" id="display-table-removed" class="comment">
                    <tr valign="top">
                        <td width="40%">One</td>
                        <td width="60%">Two</td>
                    </tr>
                </table>
                <div class="comment">
                    <p>The parent div will be deleted due to negative weight classes</p>
                </div>
                <div id="some-content">
                    The days of the week: Mon, Tue, Wed, Thur, Fri, Sat, Sun.
                    The months of the year: Jan, Feb, Mar, Apr, May, Jun, Jul, Aug, Oct, Nov, Dec.
                </div>
                <div id="embeds">
                    <iframe width="420" height="345" src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>
                </div>
                <div id="footer">
                    <p>Check out more articles</p>
                    <ul>
                        <li><img src="article.jpg"><p>Article 1</p></li>
                        <li><img src="article.jpg"><p>Article 2</p></li>
                        <li><img src="article.jpg"><p>Article 3</p></li>
                    </ul>
                </div>
            </body>
        </html>
        "#;
        let mut doc = Readability::new(html_str);
        let body = doc.root_node.select_first("body").unwrap();
        doc.mark_data_tables();
        Readability::clean_conditionally(&mut body.as_node().clone(), "table");
        assert_eq!(true, doc.root_node.select_first("#data-table").is_ok());
        assert_eq!(false, doc.root_node.select_first("#display-table").is_ok());
        assert_eq!(
            false,
            doc.root_node.select_first("#display-table-removed").is_ok()
        );
        Readability::clean_conditionally(&mut body.as_node().clone(), "div");
        assert_eq!(false, doc.root_node.select_first("div.comment").is_ok());
        assert_eq!(true, doc.root_node.select_first("div#some-content").is_ok());
        assert_eq!(true, doc.root_node.select_first("div#embeds").is_ok());
        assert_eq!(false, doc.root_node.select_first("div#footer").is_ok());
    }

    #[test]
    fn test_clean() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <pre>A Paperoni test</pre>
                <iframe width="420" height="345" src="https://www.youtube.com/embed/dQw4w9WgXcQ">
                </iframe>
                <iframe src="https://www.rust-lang.org/" name="rust_iframe" height="300px" width="100%" title="Rustlang Homepage">
                </iframe>
                <iframe src="https://crates.io/" name="crates_iframe" height="300px" width="100%" title="Crates.io Homepage">
                </iframe>
                <pre></pre>
            </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        Readability::clean(&mut doc.root_node.clone(), "pre");
        let pre_count = doc.root_node.select("pre").unwrap().count();
        assert_eq!(0, pre_count);

        Readability::clean(&mut doc.root_node.clone(), "iframe");
        let iframe_count = doc.root_node.select("iframe").unwrap().count();
        assert_eq!(1, iframe_count);
        let iframe = doc.root_node.select_first("iframe").unwrap();
        let iframe_attrs = iframe.attributes.borrow();
        assert_eq!(
            Some("https://www.youtube.com/embed/dQw4w9WgXcQ"),
            iframe_attrs.get("src")
        );
    }

    #[test]
    fn test_clean_headers() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <h1 class="tags">#blog, #rust</h1>
                <h2>A blog in Rust</h2>
                <p>Foo bar baz quux</p>
                <h1 class="footer">Copyright info</h1>
            </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let body = doc.root_node.select_first("body").unwrap();
        let h1_count = doc.root_node.select("h1").unwrap().count();
        let h2_count = doc.root_node.select("h2").unwrap().count();
        assert_eq!(2, h1_count);
        assert_eq!(1, h2_count);
        Readability::clean_headers(&mut body.as_node().clone());
        let h1_count = doc.root_node.select("h1").unwrap().count();
        let h2_count = doc.root_node.select("h2").unwrap().count();
        assert_eq!(0, h1_count);
        assert_eq!(1, h2_count);
    }

    #[test]
    fn test_clean_styles() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <div style="color:red; padding: 10px" id="red">A red box</div>
                <div height="100px" style="color:blue; padding: 10px" id="blue">
                    A blue box
                </div>
                <svg width="100" height="100">
                    <circle cx="50" cy="50" r="40" fill="green" />
                </svg>
                <table width="100%" bgcolor="yellow">
                    <tr>
                        <th>Col 1</th>
                        <th>Col 2</th>
                    </tr>
                </table>
            </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        Readability::clean_styles(&mut doc.root_node.clone());
        let red_div = doc.root_node.select_first("#red").unwrap();
        let blue_div = doc.root_node.select_first("#blue").unwrap();
        let svg = doc.root_node.select_first("svg").unwrap();
        let table = doc.root_node.select_first("table").unwrap();

        let red_div_attrs = red_div.attributes.borrow();
        let blue_div_attrs = blue_div.attributes.borrow();
        let svg_attrs = svg.attributes.borrow();
        let table_attrs = table.attributes.borrow();

        assert_eq!(1, red_div_attrs.map.len());
        assert_eq!(false, red_div_attrs.contains("style"));
        assert_eq!(2, blue_div_attrs.map.len());
        assert_eq!(false, blue_div_attrs.contains("style"));
        assert_eq!(true, blue_div_attrs.contains("height"));
        assert_eq!(2, svg_attrs.map.len());
        assert_eq!(0, table_attrs.map.len());
    }

    #[test]
    fn test_clean_matched_nodes() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <p class="example">In Rust you can have 3 kinds of variables</p>
                <ul>
                    <li class="example">Immutable</li>
                    <li class="example">Mutable</li>
                    <li class="example">Constant</li>
                </ul>
                <p>Onto more tests</p>
            </body>
        </html>
        "#;
        let doc = Readability::new(html_str);
        let body = doc.root_node.select_first("body").unwrap();
        Readability::clean_matched_nodes(&mut body.as_node().clone(), |node_ref, match_str| {
            &node_ref.as_element().unwrap().name.local == "li" && match_str.contains("example")
        });
        let p_count = doc.root_node.select("p").unwrap().count();
        let li_count = doc.root_node.select("li").unwrap().count();
        assert_eq!(2, p_count);
        assert_eq!(0, li_count);
    }

    #[test]
    fn test_prep_article() {
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <head>
                <title>A test HTML file</title>
            </head>
            <body>
                <h2>A test HTML file</h2>
                <div class="search">
                    Search for other posts
                    <input type="search" placeholder="Type here...">
                    <button id="search-btn">Search</button>
                </div>
                <aside>Some content aside</aside>
                <h1>A h1 tag</h1>
                <h1 class="banner">A h1 tag to be removed</h1>
                <table id="tbl-one"></table>
                <table width="100%" border="0" id="tbl-two">
                    <tr valign="top">
                        <td width="20%">Left</td>
                        <td height="200" width="60%">Main Content of the system</td>
                        <td width="20%">Right</td>
                    </tr>
                </table>
                <div style="color:red; padding: 10px" id="red">A red box</div>
                <div height="100px" style="color:blue; padding: 10px" id="blue">
                    A blue box
                </div>
                <svg width="100" height="100">
                    <circle cx="50" cy="50" r="40" fill="green" />
                </svg>
                <ul>
                    <li>one</li>
                    <li>two</li>
                    <li>three</li>
                </ul>
                <object data="obj.html" width="500" height="200"></object>
                <table id="tbl-three">
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
                <iframe id="yt" width="420" height="345" src="https://www.youtube.com/embed/dQw4w9WgXcQ">
                </iframe>
                <div id="foo">
                    <form action="">
                        <fieldset>
                            <legend>Personal details:</legend>
                            <label for="fname">First name:</label>
                            <input type="text" id="fname" name="fname"><br><br>
                            <label for="lname">Last name:</label>
                            <input type="text" id="lname" name="lname"><br><br>
                        </fieldset>
                    </form>
                    <br>
                    <p id="p-link">
                        omnis nemo qui libero? Eius suscipit veritatis, tenetur impedit et voluptatibus.
                        <a href="\#">Rerum repellat totam quam nobis harum fuga consequatur</a>
                        corrupti?
                    </p>
                    <br>
                    <iframe src="https://www.rust-lang.org/" name="rust_iframe" height="300px" width="100%" title="Rustlang Homepage">
                    </iframe>
                </div>
                <iframe src="https://crates.io/" name="crates_iframe" height="300px" width="100%" title="Crates.io Homepage">
                </iframe>
                <table id="tbl-replace-p">
                    <tr valign="top">
                        <td width="20%" id="td-to-p"><span>One cell table. This is going to be replaced</span></td>
                    </tr>
                </table>
                <embed type="video/webm" src="video.mp4" width="400" height="300">
                <br>
                <embed type="image/jpg" src="foo.jpg" width="300" height="200">
                <div>
                    <form action="">
                        <div>
                            <label>Join our newsletter</label>
                            <input type="email" placeholder="Your email address">
                        </div>
                        <button>Sign up</button>
                    </form>
                </div>
                <div id="div-p">
                    <p class="share">Share this as a <a href="\#">Tweet</a></p>
                    <br>
                    <p id="share">
                        Lorem ipsum dolor, sit amet consectetur adipisicing elit. Minima quia numquam aperiam dolores ipsam, eos perferendis cupiditate adipisci perspiciatis
                        dolore, sunt, iusto nobis? Nulla molestiae id repellat quibusdam nobis quia. Lorem ipsum dolor sit amet consectetur, adipisicing elit. Voluptas
                        laudantium omnis nemo qui libero? Eius suscipit veritatis, tenetur impedit et voluptatibus. Rerum repellat totam quam nobis harum fuga consequatur
                        corrupti? Lorem ipsum dolor sit amet consectetur, adipisicing elit. Iure excepturi accusamus nemo voluptatibus laborum minus dicta blanditiis totam
                        aperiam velit amet cupiditate hic a molestias odio nam, fugiat facere iusto.
                    </p>
                </div>
                <table id="tbl-replace-div">
                    <tr>
                        <td id="td-to-div"><pre>One cell table. This is going to be replaced</pre></td>
                    </tr>
                </table>
                <footer>A Paperoni test</footer>
                <footer>Copyright 2020</footer>
            </body>
        </html>
        "#;
        let mut doc = Readability::new(html_str);
        doc.article_title = "A test HTML file".into();
        let body = doc.root_node.select_first("body").unwrap();
        doc.prep_article(&mut body.as_node().clone());

        // Ensure tables were assigned their data table scores
        let table_node = doc.root_node.select_first("table").unwrap();
        let node_attr = table_node.attributes.borrow();
        assert_eq!(true, node_attr.get("readability-data-table").is_some());

        let forms_and_fieldsets = doc.root_node.select("form, fieldset").unwrap();
        assert_eq!(0, forms_and_fieldsets.count());

        let nodes = doc
            .root_node
            .select("h1, object, embed, footer, link, aside")
            .unwrap();
        assert_eq!(0, nodes.count());

        assert_eq!(2, doc.root_node.select("p").unwrap().count());
        assert_eq!(true, doc.root_node.select_first("p.share").is_err());
        assert_eq!(true, doc.root_node.select_first("p#share").is_ok());
        assert_eq!(true, doc.root_node.select_first("p#td-to-p").is_ok());

        let node = doc.root_node.select_first("h2");
        assert_eq!(true, node.is_err());

        let nodes = doc
            .root_node
            .select("input, textarea, select, button")
            .unwrap();
        assert_eq!(0, nodes.count());

        let nodes = doc.root_node.select("iframe").unwrap();
        assert_eq!(1, nodes.count());
        let node = doc.root_node.select_first("iframe#yt");
        assert_eq!(true, node.is_ok());

        let nodes = doc.root_node.select("h1").unwrap();
        assert_eq!(0, nodes.count());

        let nodes = doc
            .root_node
            .select("#tbl-one, #tbl-replace-p, #tbl-replace-div")
            .unwrap();
        assert_eq!(0, nodes.count());

        let tables = doc.root_node.select("#tbl-two, #tbl-three").unwrap();
        assert_eq!(2, tables.count());

        assert_eq!(true, doc.root_node.select_first("ul").is_ok());

        assert_eq!(2, doc.root_node.select("div").unwrap().count());
        assert_eq!(true, doc.root_node.select_first("div#div-p").is_ok());
        assert_eq!(true, doc.root_node.select_first("div#td-to-div").is_ok());

        assert_eq!(1, doc.root_node.select("br").unwrap().count());
        let node_ref = doc.root_node.select_first("br").unwrap();
        assert_eq!(
            "div",
            &node_ref
                .as_node()
                .following_siblings()
                .elements()
                .next()
                .unwrap()
                .name
                .local
        );
    }
}
