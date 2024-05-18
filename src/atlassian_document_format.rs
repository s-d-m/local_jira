use serde_json::{Map, Value};
use sqlx::types::JsonValue;

// specification of the atlassatian documentation format is available at
// https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/

fn get_content_subobject_as_vec_string(json: &Map<String, Value>) -> Result<Vec<String>, String> {
    let res = json
      .get("content")
      .and_then(|x| x.as_array())
      .and_then(|x| Some(x.iter().map(value_to_string).collect::<Vec<_>>()))
      .and_then(|x| Some(Ok(x)))
      .unwrap_or_else(|| Err(JsonValue::Object(json.clone()).to_string()));

    res
}

fn codeblock_to_string(json: &Map<String, Value>) -> String {
    let inner_content = json
        .get("content")
        .and_then(|x| x.as_array())
        .and_then(|x| Some(array_of_value_to_string(x)))
        .unwrap_or_default();

    let language = json
        .get("attrs")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("language"))
        .and_then(|x| x.as_str())
        .unwrap_or_default();

    let res = format!("```{language}\n{inner_content}\n```\n");
    res
}

fn emoji_to_string(json: &Map<String, Value>) -> String {
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

    String::from(attrs)
}

fn blockquote_to_string(json: &Map<String, Value>) -> String {
    let inner_content = match json.get("content").and_then(|x| x.as_array()) {
        None => JsonValue::Object(json.clone()).to_string(),
        Some(content) => array_of_value_to_string(content),
    };

    inner_content
        .lines()
        .map(|x| format!("> {x}"))
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default()
}

fn list_item_to_string(json: &Map<String, Value>) -> String {
    let inner_content = match get_content_subobject_as_vec_string(json) {
        Ok(value) => value,
        Err(value) => return value,
    };

    let content = inner_content
        .into_iter()
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default();

    content
}

fn bullet_list_to_string(json: &Map<String, Value>) -> String {
    let inner_content = match get_content_subobject_as_vec_string(json) {
        Ok(value) => value,
        Err(value) => return value,
    };

    let content = inner_content
        .iter()
        .map(|s| {
            let bullet_item = s
              .lines()
              .map(|x| x.trim())
              .enumerate()
              .map(|(n, s)| match n {
                  0 => format!("- {s}"),
                  _ => format!("  {s}")
              })
              .reduce(|a, b| format!("{a}\n{b}"))
              .unwrap_or_default();
            bullet_item
        })
      .reduce(|a, b| format!("{a}\n{b}"))
      .unwrap_or_default();

    format!("{content}\n")
}

fn text_to_string(json: &Map<String, Value>) -> String {
    if json.get("marks").is_some() {
        eprintln!("WARNING, marks are ignored for now");
    };

    json.get("text")
        .and_then(|x| x.as_str())
        .and_then(|x| Some(x.to_string()))
        .unwrap_or_default()
}

fn paragraph_to_string(json: &Map<String, Value>) -> String {
    let inner_content = json.get("content")
      .and_then(|x| x.as_array())
      .and_then(|x| Some(array_of_value_to_string(x)))
      .unwrap_or_default();

    inner_content
}

fn doc_to_string(json: &Map<String, Value>) -> String {
    let inner_content = json.get("content")
      .and_then(|x| x.as_array())
      .and_then(|x| Some(array_of_value_to_string(x)))
      .unwrap_or_default();

    inner_content
}


fn hardbreak_to_string(_json: &Map<String, Value>) -> String {
  String::from("\n")
}


fn heading_to_string(json: &Map<String, Value>) -> String {
    let inner_content = json.get("content")
      .and_then(|x| x.as_array())
      .and_then(|x| Some(array_of_value_to_string(x)))
      .unwrap_or_default();

    let level = json
      .get("attrs")
      .and_then(|x| x.get("level"))
      .and_then(|x| x.as_i64())
      .and_then(|x| Some(x.clamp(1, 6)))
      .unwrap_or_else(|| { 1 });

    let underline_with = | underline_char: char, inner_content: String | {
        inner_content
          .lines()
          .map(|x| {
              let len = x.len();
              let underline = underline_char.to_string().repeat(len);
              format!("{x}\n{underline}")
          })
          .reduce(|a,b| format!("{a}\n{b}"))
          .unwrap_or_default()
    };

    let to_level_1 = |inner_content: String| { underline_with('=', inner_content) };
    let to_level_2 = |inner_content: String| { underline_with('-', inner_content) };
    let to_level_n = |n: i64, inner_content: String| {
        let n: usize = n.try_into().unwrap_or(1);
        inner_content
          .lines()
          .map(|x| {
              let begin = String::from("#").repeat(n);
              format!("{begin} {x}")
          })
          .reduce(|a, b| format!("{a}\n{b}"))
          .unwrap_or_default()
    };

    match level {
        1 => to_level_1(inner_content),
        2 => to_level_2(inner_content),
        3..=6 => to_level_n(level, inner_content),
        _ => panic!("heading levels should be between 1 and 6, got {level}")
    }
}

fn inline_card_to_string(json: &Map<String, Value>) -> String {
    let repr = JsonValue::Object(json.clone()).to_string();
    format!("inline card are not supported: value is {repr}")
}

fn mention_to_string(json: &Map<String, Value>) -> String {
    let attrs = json
      .get("attrs")
      .and_then(|x| x.as_object());
    let Some(attrs) = attrs else {
        return JsonValue::Object(json.clone()).to_string();
    };

    let text = attrs.get("text")
      .and_then(|x| x.as_str());

    if let Some(s) = text {
        return String::from(s);
    }

    let id = attrs.get("id")
      .and_then(|x| x.as_str());

    match id {
        None => { JsonValue::Object(json.clone()).to_string() }
        Some(s) => { String::from(s) }
    }
}

fn ordered_list_to_string(json: &Map<String, Value>) -> String {
    let content = json
      .get("content")
      .and_then(|x| x.as_array());

    let Some(content) = content else {
        return JsonValue::Object(json.clone()).to_string();
    };

    let init_num = json
      .get("attrs")
      .and_then(|x| x.as_object())
      .and_then(|x| x.get("order"))
      .and_then(|x| x.as_u64())
      .unwrap_or(1);

    let content = content
      .into_iter()
      .map(|x| root_elt_doc_to_string(x))
      .collect::<Vec<_>>();

    content
      .iter()
      .enumerate()
      .map(|(n, s)| format!("{pos}. {s}", pos = u64::try_from(n).unwrap_or(0) + init_num))
      .reduce(|a, b| format!("{a}\n{b}"))
      .unwrap_or_else(|| JsonValue::Object(json.clone()).to_string())
}

fn panel_to_string(json: &Map<String, Value>) -> String {
    let panel_type = json
      .get("attrs")
      .and_then(|x| x.as_object())
      .and_then(|x| x.get("panelType"))
      .and_then(|x| x.as_str());

    let panel_type = match panel_type {
        Some(x) if (x == "info") || (x == "note") || (x == "warning") || (x == "success") || (x == "error") => x,
        _ => return JsonValue::Object(json.clone()).to_string(),
    };

    let content = json
      .get("content")
      .and_then(|x| x.as_array())
      .and_then(|x| Some(array_of_value_to_string(x)));

    let content = match content {
        None => { return JsonValue::Object(json.clone()).to_string(); }
        Some(value) => { value }
    };

    let content = content
      .lines()
      .map(|x| format!("  {x}"))
      .reduce(|a, b| format!("{a}\n{b}"));

    let content = match content {
        None => { return JsonValue::Object(json.clone()).to_string(); }
        Some(value) => { value }
    };

    format!("{panel_type}:\n{content}")

}

fn rule_to_string(_json: &Map<String, Value>) -> String {
    String::from("\n")
}

fn object_to_string(json: &Map<String, Value>) -> String {
    let Some(type_elt) = json.get("type").and_then(|x| x.as_str()) else {
        return JsonValue::Object(json.clone()).to_string();
    };

    match type_elt {
        "blockquote" => blockquote_to_string(json),
        "bulletList" => bullet_list_to_string(json),
        "codeBlock" => codeblock_to_string(json),
        "doc" => doc_to_string(json),
        "emoji" => emoji_to_string(json),
        "hardBreak" => hardbreak_to_string(json),
        "heading" => heading_to_string(json),
        // "inlineCard" => inlinecard_to_string(json),
        "listItem" => list_item_to_string(json),
        // media => todo!(),
        "mention" => mention_to_string(json),
        "orderedList" => ordered_list_to_string(json),
        "panel" => panel_to_string(json),
        "paragraph" => paragraph_to_string(json),
        "rule" => rule_to_string(json),
        // table
        "text" => text_to_string(json),
        _ => JsonValue::Object(json.clone()).to_string(),
    }
}

enum NodeLevel {
  TopLevel,
    Inline,
    
}

struct string_with_node_level {
    text: String,
    node_level: NodeLevel,
}

fn value_to_string(json: &JsonValue) -> String {
    match json {
        Value::Null => String::from("null"),
        Value::Bool(n) => n.to_string(),   // String::from(n),
        Value::Number(n) => n.to_string(), // String::from(n),
        Value::String(n) => String::from(n),
        Value::Array(n) => array_of_value_to_string(n),
        Value::Object(o) => object_to_string(o),
    }
}

fn array_of_value_to_string(content: &Vec<JsonValue>) -> String {
    let res = content
        .iter()
        .map(|x| value_to_string(x))
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default();

    res
}

pub(crate) fn root_elt_doc_to_string(description: &JsonValue) -> String {
    let res = description
      .as_object()
      .and_then(|x| x.get("content"))
      .and_then(|x| x.as_array())
      .and_then(|x| Some(array_of_value_to_string(x)))
      .unwrap_or_else(|| description.to_string());

    res
}
