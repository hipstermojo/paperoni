use std::collections::BTreeMap;

use crate::extractor::MetaAttr;

use html5ever::{LocalName, Namespace, QualName};
use kuchiki::{
    iter::{Descendants, Elements, Select},
    traits::*,
    NodeData, NodeRef,
};

const HTML_NS: &'static str = "http://www.w3.org/1999/xhtml";
const PHRASING_ELEMS: [&str; 39] = [
    "abbr", "audio", "b", "bdo", "br", "button", "cite", "code", "data", "datalist", "dfn", "em",
    "embed", "i", "img", "input", "kbd", "label", "mark", "math", "meter", "noscript", "object",
    "output", "progress", "q", "ruby", "samp", "script", "select", "small", "span", "strong",
    "sub", "sup", "textarea", "time", "var", "wbr",
];

pub struct Readability {
    root_node: NodeRef,
}

impl Readability {
    pub fn new(html_str: &str) -> Self {
        Self {
            root_node: kuchiki::parse_html().one(html_str),
        }
    }
    pub fn parse(&mut self) {
        self.unwrap_no_script_tags();
        self.remove_scripts();
        self.prep_document();
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
                        // TODO: Replace the code below with next_element
                        prev_elem.insert_after(
                            inner_node_ref
                                .first_child()
                                .unwrap()
                                .children()
                                .filter(Self::has_content)
                                .next()
                                .unwrap(),
                        );
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
            // TODO: Change to for loop
            while let Some(br_tag) = br_tags.next() {
                let mut next = br_tag.as_node().next_sibling();
                let mut replaced = false;
                Self::next_element(&mut next);
                while let Some(next_elem) = next {
                    if next_elem.as_element().is_some()
                        && &next_elem.as_element().as_ref().unwrap().name.local == "br"
                    {
                        replaced = true;
                        let br_sibling = next_elem.next_sibling();
                        next_elem.detach();
                        next = br_sibling;
                    } else {
                        break;
                    }
                    Self::next_element(&mut next);
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
    fn next_element(node_ref: &mut Option<NodeRef>) {
        // The signature of this method affects how it is used in loops since the code
        // has to ensure that it is called before the next iteration. This probably
        // makes it less obvious to understand the first time. This may change in the future.
        while node_ref.is_some() {
            match node_ref.as_ref().unwrap().data() {
                NodeData::Element(_) => break,
                _ => {
                    if node_ref.as_ref().unwrap().text_contents().trim().is_empty() {
                        *node_ref = node_ref.as_ref().unwrap().next_sibling();
                    } else {
                        break;
                    }
                }
            }
        }
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

    /// Using a variety of metrics (content score, classname, element types), find the content that is most likely to be the stuff
    /// a user wants to read. Then return it wrapped up in a div.
    fn grab_article(&mut self) {}
}

#[cfg(test)]
mod test {
    use super::Readability;
    use super::HTML_NS;
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
        Readability::next_element(&mut p_node_option);
        assert_eq!(Some(p.clone()), p_node_option);

        let p_node_option = p_node_option.unwrap();
        let p_node_option = p_node_option.as_element();
        let p_node_option_attr = p_node_option.unwrap().attributes.borrow();
        assert_eq!("a", p_node_option_attr.get("id").unwrap());

        let mut next = p.next_sibling();
        Readability::next_element(&mut next);

        let next = next.unwrap();
        let next_elem = next.as_element();
        let next_attr = next_elem.unwrap().attributes.borrow();
        assert_eq!("b", next_attr.get("id").unwrap());

        let mut next = next.next_sibling();
        Readability::next_element(&mut next);

        let next = next.unwrap();
        assert_eq!(true, next.as_text().is_some());
        assert_eq!("This is standalone text", next.text_contents().trim());

        let mut next = None;
        Readability::next_element(&mut next);
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
}
