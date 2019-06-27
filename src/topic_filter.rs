use std::fmt;

use regex::{escape, Regex, Captures};
use snafu::{Snafu, ResultExt};

lazy_static!{
    static ref VALIDATOR: Regex = Regex::new(r"^(([^/\+#]*|\+)/)*([^/\+#]*|#|\+)?$").unwrap();
    static ref REPLACER: Regex = Regex::new("([^/]*)").unwrap();
}

static SINGLE_WILDCARD_RE: &'static str = "([^/]+)";
static MULTI_WILDCARD_RE: &'static str = "?(.*)";

#[derive(Snafu, Debug, PartialEq)]
pub enum Error<'a> {
    #[snafu(display("Topic filter cannot be empty."))]
    EmptyTopicFilter,
    #[snafu(display("Invalid topic filter '{}'.", filter))]
    InvalidTopicFilter{ filter: &'a str },
    #[snafu(display("Unable to compile topic filter '{}': {}", filter, source))]
    CompilationError{ filter: &'a str, source: regex::Error },
}

#[derive(Debug)]
pub struct TopicFilter {
    filter: String,
    matcher: Regex,
}

impl TopicFilter {
    pub fn new<'a>(s: &'a str) -> Result<TopicFilter, Error<'a>> {
        ensure!(!s.is_empty(), EmptyTopicFilter);
        ensure!(VALIDATOR.is_match(s), InvalidTopicFilter{ filter: s });

        let match_expr = REPLACER.replace_all(s, |caps: &Captures| {
            let part = caps.get(0).unwrap().as_str();
            if part == "+" {
                SINGLE_WILDCARD_RE.into()
            } else if part == "#" {
                MULTI_WILDCARD_RE.into()
            } else {
                escape(part)
            }
        });

        let match_regex = Regex::new(&match_expr).with_context(|| CompilationError { filter: s.clone() })?;
        Ok(TopicFilter {
            filter: s.into(),
            matcher: match_regex
        })
    }

    pub fn match_topic(&self, topic: &str) -> bool {
        self.matcher.is_match(topic)
    }
}

impl fmt::Display for TopicFilter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_topic_filter() {
        assert_eq!(TopicFilter::new("this/is/#/invalid").unwrap_err(),
            Error::InvalidTopicFilter{ filter: "this/is/#/invalid".into() });
        assert_eq!(TopicFilter::new("invalid/filter#").unwrap_err(),
            Error::InvalidTopicFilter{ filter: "invalid/filter#".into() });
        assert_eq!(TopicFilter::new("this/is+/invalid").unwrap_err(),
            Error::InvalidTopicFilter{ filter: "this/is+/invalid".into() });
        assert_eq!(TopicFilter::new("another/+wrong/one").unwrap_err(),
            Error::InvalidTopicFilter{ filter: "another/+wrong/one".into() });
    }

    #[test]
    fn simple_filter() {
        let topic = "this/is/a/filter";
        let filter = TopicFilter::new(topic).unwrap();
        assert!(filter.match_topic(topic));
        assert!(!filter.match_topic("this/is/wrong"));
        assert!(!filter.match_topic("/this/is/a/filter"));
    }

    #[test]
    fn single_level_filter() {
        let filter_str = "this/is/+/level";
        let filter = TopicFilter::new(filter_str).unwrap();
        assert!(filter.match_topic("this/is/single/level"));
        assert!(!filter.match_topic("this/is/not/valid/level"));
    }

    #[test]
    fn complex_single_level_filter() {
        let filter_str = "+/multi/+/+";
        let filter = TopicFilter::new(filter_str).unwrap();
        assert!(filter.match_topic("anything/multi/foo/bar"));
        assert!(!filter.match_topic("not/multi/valid"));
    }
}
