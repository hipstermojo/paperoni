/// This module contains regular expressions frequently used by moz_readability
/// All regexes that only test if a `&str` matches the regex are preceded by the
/// word "is_match". All other regexes are publicly accessible.
use regex::Regex;
pub fn is_match_byline(match_str: &str) -> bool {
    lazy_static! {
        static ref BYLINE_REGEX: Regex =
            Regex::new(r"(?i)byline|author|dateline|writtenby|p-author").unwrap();
    }
    BYLINE_REGEX.is_match(match_str)
}

pub fn is_match_positive(match_str: &str) -> bool {
    lazy_static! {
        static ref POSITIVE_REGEX: Regex = Regex::new(r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story").unwrap();
    }
    POSITIVE_REGEX.is_match(match_str)
}

pub fn is_match_negative(match_str: &str) -> bool {
    lazy_static! {
        static ref NEGATIVE_REGEX: Regex = Regex::new(r"(?i)hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|foot|footer|footnote|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|tool|widget").unwrap();
    }
    NEGATIVE_REGEX.is_match(match_str)
}

pub fn is_match_videos(match_str: &str) -> bool {
    lazy_static! {
        static ref VIDEOS_REGEX: Regex = Regex::new(r"(?i)//(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)").unwrap();
    }
    VIDEOS_REGEX.is_match(match_str)
}

pub fn is_match_unlikely(match_str: &str) -> bool {
    lazy_static! {
        static ref UNLIKELY_REGEX: Regex = Regex::new(r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote").unwrap();
    }
    UNLIKELY_REGEX.is_match(match_str)
}

pub fn is_match_ok_maybe(match_str: &str) -> bool {
    lazy_static! {
        static ref OK_MAYBE_REGEX: Regex =
            Regex::new(r"(?i)and|article|body|column|content|main|shadow").unwrap();
    }
    OK_MAYBE_REGEX.is_match(match_str)
}

pub fn is_match_node_content(match_str: &str) -> bool {
    lazy_static! {
        static ref NODE_CONTENT_REGEX: Regex = Regex::new(r"\.( |$)").unwrap();
    }
    NODE_CONTENT_REGEX.is_match(match_str)
}

pub fn is_match_share_elems(match_str: &str) -> bool {
    lazy_static! {
        static ref SHARE_ELEMS_REGEX: Regex =
            Regex::new(r"(?i)(\b|_)(share|sharedaddy)(\b|_)").unwrap();
    }
    SHARE_ELEMS_REGEX.is_match(match_str)
}

pub fn is_match_has_content(match_str: &str) -> bool {
    lazy_static! {
        static ref HAS_CONTENT_REGEX: Regex = Regex::new(r"\S$").unwrap();
    }
    HAS_CONTENT_REGEX.is_match(match_str)
}

pub fn is_match_img_ext(match_str: &str) -> bool {
    lazy_static! {
        static ref IMG_EXT_REGEX: Regex = Regex::new(r"(?i)\.(jpg|jpeg|png|webp)").unwrap();
    }
    IMG_EXT_REGEX.is_match(match_str)
}

pub fn is_match_srcset(match_str: &str) -> bool {
    lazy_static! {
        static ref SRCSET_REGEX: Regex = Regex::new(r"\.(jpg|jpeg|png|webp)\s+\d").unwrap();
    }
    SRCSET_REGEX.is_match(match_str)
}

pub fn is_match_src_regex(match_str: &str) -> bool {
    lazy_static! {
        static ref SRC_REGEX: Regex = Regex::new(r"^\s*\S+\.(jpg|jpeg|png|webp)\S*\s*$").unwrap();
    }
    SRC_REGEX.is_match(match_str)
}

pub fn is_match_name_pattern(match_str: &str) -> bool {
    lazy_static! {
        static ref NAME_PATTERN_REGEX: Regex = Regex::new(r"(?i)\s*(?:(dc|dcterm|og|twitter|weibo:(article|webpage))\s*[\.:]\s*)?(author|creator|description|title|site_name)\s*$").unwrap();
    }
    NAME_PATTERN_REGEX.is_match(match_str)
}

pub fn is_match_title_separator(match_str: &str) -> bool {
    lazy_static! {
        static ref TITLE_SEPARATOR_REGEX: Regex = Regex::new(r" [\|\-\\/>»] ").unwrap();
    }
    TITLE_SEPARATOR_REGEX.is_match(match_str)
}

pub fn is_match_has_title_separator(match_str: &str) -> bool {
    lazy_static! {
        static ref HAS_TITLE_SEPARATOR_REGEX: Regex = Regex::new(r" [\\/>»] ").unwrap();
    }
    HAS_TITLE_SEPARATOR_REGEX.is_match(match_str)
}

lazy_static! {
    pub static ref NORMALIZE_REGEX: Regex = Regex::new(r"\s{2,}").unwrap();
    pub static ref B64_DATA_URL_REGEX: Regex =
        Regex::new(r"(?i)^data:\s*([^\s;,]+)\s*;\s*base64\s*").unwrap();
    pub static ref BASE64_REGEX: Regex = Regex::new(r"(?i)base64\s*").unwrap();
    pub static ref PROPERTY_REGEX: Regex = Regex::new(
        r"(?i)\s*(dc|dcterm|og|twitter)\s*:\s*(author|creator|description|title|site_name)\s*"
    )
    .unwrap();
    pub static ref REPLACE_WHITESPACE_REGEX: Regex = Regex::new(r"\s").unwrap();
    pub static ref REPLACE_DOT_REGEX: Regex = Regex::new(r"\.").unwrap();
    pub static ref REPLACE_HTML_ESCAPE_REGEX: Regex =
        Regex::new("&(quot|amp|apos|lt|gt);").unwrap();
    pub static ref REPLACE_HEX_REGEX: Regex =
        Regex::new(r"(?i)&#(?:x([0-9a-z]{1,4})|([0-9]{1,4}));").unwrap();
    pub static ref REPLACE_START_SEPARATOR_REGEX: Regex =
        Regex::new(r"(?i)(?P<start>.*)[\|\-\\/>»] .*").unwrap();
    pub static ref REPLACE_END_SEPARATOR_REGEX: Regex =
        Regex::new(r"(?i)[^\|\-\\/>»]*[\|\-\\/>»](?P<end>.*)").unwrap();
    pub static ref REPLACE_MULTI_SEPARATOR_REGEX: Regex = Regex::new(r"[\|\-\\/>»]+").unwrap();
}
