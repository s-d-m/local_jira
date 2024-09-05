use serde_json::{Map, Value};

#[derive(Copy, Clone, Debug)]
pub enum NodeLevel {
  TopLevel,
  ChildNode,
  Inline,
}

#[derive(Debug)]
pub(crate) struct StringWithNodeLevel {
  pub text: String,
  pub node_level: NodeLevel,
}

pub(crate) fn to_inline(content: String) -> StringWithNodeLevel {
  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::Inline,
  }
}

pub(crate) fn to_top_level(content: String) -> StringWithNodeLevel {
  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  }
}


pub(crate) fn indent_with(text: &str, lines_starter: &str) -> String {
  text.lines()
    .map(|x| format!("{lines_starter}{x}"))
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default()
}

pub(crate) fn emoji_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  let attrs = json
    .get("attrs")
    .and_then(|x| {
      if let Some(x) = x.get("text") {
        x.as_str()
      } else {
        x.get("shortName").and_then(|x| x.as_str())
      }
    })
    .unwrap_or_default();

  let res = String::from(attrs);
  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::Inline,
  }
}

pub(crate) struct LinkAttrs {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/link/
  pub collection: Option<String>,
  pub href: String,
  pub id: Option<String>,
  pub occurrenceKey: Option<String>,
  pub title: Option<String>,
}

pub(crate) enum MarkKind {
  BackgroundColour(String), // Html hexa colour. e.g. #daa520 https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/textColor/
  Code,
  Emphasis, // aka italics
  Link(LinkAttrs),
  Strike,
  Strong,
  Superscript,
  SubScript,
  TextColour(String), // Html hexa colour. e.g. #daa520 https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/textColor/
  Underline,
}


pub(crate) fn get_html_colour_from_mark(colour_kind: &Map<String, Value>) -> Result<String, String> {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/textColor/
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/backgroundColor/

  let Some(attrs) = colour_kind.get("attrs") else {
    return Err(String::from(
      "Error: colour mark does not have an attrs array",
    ));
  };

  let Some(attrs) = attrs.as_object() else {
    return Err(String::from(
      "Error: colour mark attrs attribute is not a json object.",
    ));
  };

  let Some(colour) = attrs.get("color") else {
    return Err(String::from(
      "Error: colour mark attrs attribute does not contain a href element",
    ));
  };

  let Some(colour) = colour.as_str() else {
    return Err(String::from("Error: colour mark element is not a string"));
  };

  if colour.len() != 7 {
    return Err(String::from(
      "Error: colour attribute is not an html hexadecimal colour (wrong length)",
    ));
  }

  let chars = colour.as_bytes();
  if chars[0] != ('#' as u8) {
    return Err(String::from(
      "Error: colour attribute is not an html hexadecimal colour (doesn't starts by #)",
    ));
  }

  for i in 1..=6 {
    if !chars[i].is_ascii_hexdigit() {
      return Err(String::from(
        "Error: colour attribute is not an html hexadecimal colour (not hexa value)",
      ));
    }
  }

  let res = Ok(String::from(colour));
  res
}

pub(crate) fn get_background_colour_mark_kind(colour_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  let res = get_html_colour_from_mark(colour_kind);
  match res {
    Ok(s) => Ok(MarkKind::BackgroundColour(s)),
    Err(e) => Err(e),
  }
}

pub(crate) fn get_text_colour_mark_kind(colour_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  let res = get_html_colour_from_mark(colour_kind);
  match res {
    Ok(s) => Ok(MarkKind::TextColour(s)),
    Err(e) => Err(e),
  }
}

pub(crate) fn get_link_mark_kind(link_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/link/
  let Some(attrs) = link_kind.get("attrs") else {
    return Err(String::from(
      "Error: link mark does not have an attrs array",
    ));
  };

  let Some(attrs) = attrs.as_object() else {
    return Err(String::from(
      "Error: link mark attrs attribute is not a json object.",
    ));
  };

  let Some(href) = attrs.get("href") else {
    return Err(String::from(
      "Error: link mark attrs attribute does not contain a href element",
    ));
  };

  let Some(href) = href.as_str() else {
    return Err(String::from(
      "Error: link mark href element is not a string",
    ));
  };

  let collection = attrs.get("collection");
  let id = attrs.get("id");
  let occurrenceKey = attrs.get("occurrenceKey");
  let title = attrs.get("title");

  let to_option_string = |value: Option<&Value>| {
    value
      .and_then(|x| x.as_str())
      .and_then(|x| Some(x.to_string()))
  };
  let collection = to_option_string(collection);
  let id = to_option_string(id);
  let occurrenceKey = to_option_string(occurrenceKey);
  let title = to_option_string(title);

  let href = href.to_string();

  let res = MarkKind::Link(LinkAttrs {
    collection,
    href,
    id,
    occurrenceKey,
    title,
  });

  Ok(res)
}

pub(crate) fn get_sub_sup_mark_kind(subsup_mark: &Map<String, Value>) -> Result<MarkKind, String> {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/subsup/
  let Some(attrs) = subsup_mark.get("attrs") else {
    return Err(String::from(
      "Error: subsup mark does not have an attrs array",
    ));
  };

  let Some(attrs) = attrs.as_object() else {
    return Err(String::from(
      "Error: subsup mark attrs attribute is not a json object.",
    ));
  };

  let Some(subsup) = attrs.get("subsup") else {
    return Err(String::from(
      "Error: subsup mark attrs attribute does not contain a subsup element",
    ));
  };

  let Some(subsup) = subsup.as_str() else {
    return Err(String::from("Error: subsup mark element is not a string"));
  };

  match subsup {
    "sub" => Ok(MarkKind::SubScript),
    "sup" => Ok(MarkKind::Superscript),
    _ => Err(String::from("Error subsup value is neither sub nor sup")),
  }
}

pub(crate) fn get_mark_kind(mark: &Value) -> Result<MarkKind, String> {
  let Some(mark) = mark.as_object() else {
    return Err(String::from("Invalid mark. Expecting json object"));
  };

  let Some(kind) = mark.get("type") else {
    return Err(String::from(
      "Invalid mark kind. Object doesn't have a type",
    ));
  };

  let Some(kind) = kind.as_str() else {
    return Err(String::from("Invalid mark kind. Type isn't a string"));
  };

  match kind {
    "backgroundColor" => get_background_colour_mark_kind(mark),
    "code" => Ok(MarkKind::Code),
    "em" => Ok(MarkKind::Emphasis),
    "link" => get_link_mark_kind(mark),
    "strike" => Ok(MarkKind::Strike),
    "strong" => Ok(MarkKind::Strong),
    "subsup" => get_sub_sup_mark_kind(mark),
    "textColor" => get_text_colour_mark_kind(mark),
    "underline" => Ok(MarkKind::Underline),
    _ => Err(format!("Unknown kind of mark. Got {kind}")),
  }
}