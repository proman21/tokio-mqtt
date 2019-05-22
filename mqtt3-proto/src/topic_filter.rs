use ::regex::{escape, Regex};
use ::failure::{Fallible, ensure, format_err};

lazy_static!{
    static ref INVALID_MULTILEVEL: Regex = Regex::new("(?:[^/]#|#(?:.+))").unwrap();
    static ref INVALID_SINGLELEVEL: Regex = Regex::new(r"(?:[^/]\x2B|\x2B[^/])").unwrap();
}

pub struct TopicFilter {
    matcher: Regex,
}

impl TopicFilter {
    pub fn from_string(s: &str) -> Fallible<TopicFilter> {
        // See if topic is legal
        ensure!(!(INVALID_SINGLELEVEL.is_match(s) || INVALID_MULTILEVEL.is_match(s)),
            "Invalid topic filter.");
        ensure!(!s.is_empty(), "Invalid topic filter.");

        let mut collect: Vec<String> = Vec::new();
        for tok in s.split("/") {
            if tok.contains("+") {
                collect.push(String::from("[^/]+"));
            } else if tok.contains("#") {
                collect.push(String::from("?.*"));
            } else {
                collect.push(escape(tok))
            }
        }
        let match_expr = format!("^{}$", collect.join("/"));
        let match_regex = Regex::new(&match_expr).map_err(|_| format_err!("Invalid topic filter."))?;
        Ok(TopicFilter {
            matcher: match_regex
        })
    }

    pub fn match_topic(&self, topic: &str) -> bool {
        self.matcher.is_match(topic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_filter() {
        let topic = "this/is/#/invalid";
        let topic2 = "invalid/filter#";
        let res = TopicFilter::from_string(topic);
        assert!(res.is_err());
        let res2 = TopicFilter::from_string(topic2);
        assert!(res2.is_err());
    }

    #[test]
    fn simple_filter() {
        let topic = "this/is/a/filter";
        let res = TopicFilter::from_string(topic);
        assert!(res.is_ok());
        let filter = res.unwrap();
        assert!(filter.match_topic(topic));
        assert!(!filter.match_topic("this/is/wrong"));
        assert!(!filter.match_topic("/this/is/a/filter"));
    }

    #[test]
    fn single_level_filter() {
        let filter_str = "this/is/+/level";
        let res = TopicFilter::from_string(filter_str);
        assert!(res.is_ok());
        let filter = res.unwrap();
        assert!(filter.match_topic("this/is/single/level"));
        assert!(!filter.match_topic("this/is/not/valid/level"));
    }

    #[test]
    fn complex_single_level_filter() {
        let filter_str = "+/multi/+/+";
        let res = TopicFilter::from_string(filter_str);
        assert!(res.is_ok());
        let filter = res.unwrap();
        assert!(filter.match_topic("anything/multi/foo/bar"));
        assert!(!filter.match_topic("not/multi/valid"));
    }
}
