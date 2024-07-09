use crate::error::{new_regex_error, Result};
use crate::info::Version;
use crate::matchers::Part;
use crate::serde_format::{is_default, part_serde};
use jsonpath_rust::JsonPathInst;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Extractor {
  #[serde(default, skip_serializing_if = "is_default")]
  pub name: Option<String>,
  #[serde(with = "part_serde", default, skip_serializing_if = "is_default")]
  pub part: Part,
  #[serde(flatten)]
  pub extractor_type: ExtractorType,
  #[serde(default, skip_serializing_if = "is_default")]
  pub internal: bool,
  #[serde(default, skip_serializing_if = "is_default")]
  pub case_insensitive: bool,
  /// 预编译正则
  #[serde(skip)]
  pub regex: Vec<fancy_regex::Regex>,
}

impl PartialEq for Extractor {
  fn eq(&self, other: &Self) -> bool {
    self.name == other.name
      && self.part == other.part
      && self.extractor_type == other.extractor_type
      && self.internal == other.internal
      && self.case_insensitive == other.case_insensitive
  }
}

impl Extractor {
  pub(crate) fn compile(&mut self) -> Result<()> {
    if let ExtractorType::Regex(regexps) = &self.extractor_type {
      for re in regexps.regex.iter() {
        let rec = fancy_regex::Regex::new(re).map_err(new_regex_error)?;
        self.regex.push(rec);
      }
    }
    Ok(())
  }
  pub fn extrat_json(
    &self,
    json_path: &JsonPath,
    corpus: String,
  ) -> (HashSet<String>, HashMap<String, String>) {
    let mut extract_result = HashSet::new();
    let json = if let Ok(x) = serde_json::from_str(&corpus) {
      x
    } else {
      return (extract_result, HashMap::new());
    };
    for path in json_path.json.iter() {
      if let Ok(p) = JsonPathInst::from_str(path) {
        if let serde_json::Value::Array(array) = jsonpath_rust::find(&p, &json) {
          for v in array {
            extract_result.insert(v.to_string());
          }
        };
      }
    }
    (extract_result, HashMap::new())
  }
  pub(crate) fn extract_regex(
    &self,
    regexps: &ERegex,
    corpus: String,
    version: &Option<Version>,
  ) -> (HashSet<String>, HashMap<String, String>) {
    let mut extract_result = HashSet::new();
    let mut version_map = HashMap::new();
    let group = regexps.group.unwrap_or(0);
    for re in self.regex.iter() {
      re.captures(&corpus)
        .map(|e| {
          e.map(|e| {
            if let Some(eg) = e.get(group) {
              extract_result.insert(eg.as_str().to_string());
            }
            if let Some(ver) = version {
              version_map = ver.captures(e);
            }
          })
        })
        .unwrap_or_default();
    }
    (extract_result, version_map)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum ExtractorType {
  // name:regex
  Regex(ERegex),
  // name:kval
  KVal(KVal),
  // name:xpath
  XPath(XPath),
  // name:json
  JSON(JsonPath),
  // name:dsl
  DSL(DSL),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct ERegex {
  pub regex: Vec<String>,
  #[serde(default, skip_serializing_if = "is_default")]
  pub group: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct KVal {
  pub group: Option<i32>,
  pub kval: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct JsonPath {
  pub group: Option<i32>,
  pub json: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct XPath {
  pub xpath: HashSet<String>,
  pub attribute: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct DSL {
  pub dsl: HashSet<String>,
}
