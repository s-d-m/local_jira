use serde_json::{Map, Value};
use sqlx::types::JsonValue;

// specification of the atlassatian documentation format is available at
// https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/

fn get_content_subobject_as_vec_string(json: &Map<String, Value>) -> Result<Vec<String>, String> {
    let inner_content = match json.get("content").and_then(|x| x.as_array()) {
        None => {
            return Err(JsonValue::Object(json.clone()).to_string());
        }
        Some(content) => content.iter().map(value_to_string).collect::<Vec<_>>(),
    };
    Ok(inner_content)
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
            let bullet_item = s.trim().lines().collect::<Vec<_>>();
            match bullet_item.len() {
                0 => String::from(""),
                _ => {
                    let first_line = format!("- {x}", x = bullet_item[0]);
                    let rest = bullet_item
                        .iter()
                        .skip(1)
                        .map(|x| format!("  {x}"))
                        .reduce(|a, b| format!("{a}\n{b}"));
                    match rest {
                        None => first_line,
                        Some(s) => {
                            format!("{first_line}\n{s}")
                        }
                    }
                }
            }
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
    let inner_content = match json.get("content").and_then(|x| x.as_array()) {
        None => "".to_string(),
        Some(content) => array_of_value_to_string(content),
    };

    format!("\n{inner_content}")
}

fn object_to_string(json: &Map<String, Value>) -> String {
    let Some(type_elt) = json.get("type").and_then(|x| x.as_str()) else {
        return JsonValue::Object(json.clone()).to_string();
    };

    match type_elt {
        "blockquote" => blockquote_to_string(json),
        "text" => text_to_string(json),
        "paragraph" => paragraph_to_string(json),
        "bulletList" => bullet_list_to_string(json),
        "listItem" => list_item_to_string(json),
        "codeBlock" => codeblock_to_string(json),
        _ => JsonValue::Object(json.clone()).to_string(),
    }
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

pub(crate) fn adf_doc_to_string(description: &JsonValue) -> String {
    let Some(json) = description.as_object() else {
        return description.to_string();
    };

    let doc_json_str = serde_json::json!("doc");
    let Some(doc_json_str) = json.get("type") else {
        return description.to_string();
    };

    let Some(json) = json.get("content").and_then(|x| x.as_array()) else {
        return description.to_string();
    };

    array_of_value_to_string(json)
}
