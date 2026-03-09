use crate::models::{DesignOutput, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntentClassification {
    pub intent: String, // "question" | "design"
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub response: String,
}

pub enum ResponseFormat {
    DesignOutput,
    JsonObject,
}

pub async fn generate_design(
    engine: &Engine,
    prompt: &str,
    images: Vec<String>,
) -> Result<DesignOutput, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match engine.provider.as_str() {
        "openai" | "ollama" => {
            let res = call_openai_compatible(
                &client,
                engine,
                engine.model.as_str(),
                &engine.system_prompt,
                prompt,
                images,
                ResponseFormat::DesignOutput,
            )
            .await?;
            serde_json::from_value(res).map_err(|e| e.to_string())
        }
        "gemini" => {
            let res = call_gemini(
                &client,
                engine,
                engine.model.as_str(),
                &engine.system_prompt,
                prompt,
                images,
                ResponseFormat::DesignOutput,
            )
            .await?;
            serde_json::from_value(res).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unsupported provider: {}", engine.provider)),
    }
}

pub async fn classify_intent(
    engine: &Engine,
    prompt: &str,
    context: Option<&str>,
    images: Vec<String>,
) -> Result<IntentClassification, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let light_model = if engine.light_model.trim().is_empty() {
        engine.model.as_str()
    } else {
        engine.light_model.as_str()
    };

    let classifier_system = r#"Return ONLY JSON with fields:
1) "intent": "question" or "design"
2) "confidence": number in [0, 1]
3) "response": text reply.

Choose "question" when user asks to explain, inspect, compare, clarify, or asks "why/how/what" about existing design/code.
Choose "design" when user asks to create/change/add/remove geometry, parameters, dimensions, connectors, or regenerate output.

If intent is "question", "response" must directly answer the user's question in 1-4 concise sentences using the provided current design context and screenshots when relevant.
If intent is "design", "response" must be one short routing sentence for the assistant bubble.
"#;

    let classifier_user = if let Some(context) = context.filter(|c| !c.trim().is_empty()) {
        format!(
            "CURRENT DESIGN CONTEXT:\n{}\n\nUSER REQUEST:\n{}",
            context, prompt
        )
    } else {
        format!("USER REQUEST:\n{}", prompt)
    };

    let raw: serde_json::Value = match engine.provider.as_str() {
        "openai" | "ollama" => {
            call_openai_compatible(
                &client,
                engine,
                light_model,
                classifier_system,
                &classifier_user,
                images,
                ResponseFormat::JsonObject,
            )
            .await?
        }
        "gemini" => {
            call_gemini(
                &client,
                engine,
                light_model,
                classifier_system,
                &classifier_user,
                images,
                ResponseFormat::JsonObject,
            )
            .await?
        }
        _ => return Err(format!("Unsupported provider: {}", engine.provider)),
    };

    let mut parsed: IntentClassification =
        serde_json::from_value(raw).map_err(|e| format!("Intent parse error: {}", e))?;
    parsed.intent = parsed.intent.to_lowercase();
    if parsed.intent != "question" && parsed.intent != "design" {
        parsed.intent = "design".to_string();
    }
    if !(0.0..=1.0).contains(&parsed.confidence) {
        parsed.confidence = 0.5;
    }
    if parsed.response.trim().is_empty() {
        parsed.response = if parsed.intent == "question" {
            "Thinking not deep enough. Treating this as a question.".to_string()
        } else {
            "Intent looks like a design change request.".to_string()
        };
    }
    Ok(parsed)
}

pub async fn list_models(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match provider {
        "openai" => fetch_openai_models(&client, api_key, base_url).await,
        "gemini" => fetch_gemini_models(&client, api_key).await,
        _ => Ok(vec![]),
    }
}

fn openai_chat_completions_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return "https://api.openai.com/v1/chat/completions".to_string();
    }
    if normalized.ends_with("/chat/completions") {
        return normalized.to_string();
    }
    if normalized.ends_with("/responses") {
        return format!("{}/chat/completions", normalized.trim_end_matches("/responses"));
    }
    if normalized.ends_with("/models") {
        return format!("{}/chat/completions", normalized.trim_end_matches("/models"));
    }
    format!("{}/chat/completions", normalized)
}

fn openai_models_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return "https://api.openai.com/v1/models".to_string();
    }
    if normalized.ends_with("/models") {
        return normalized.to_string();
    }
    if normalized.ends_with("/chat/completions") {
        return format!("{}/models", normalized.trim_end_matches("/chat/completions"));
    }
    if normalized.ends_with("/responses") {
        return format!("{}/models", normalized.trim_end_matches("/responses"));
    }
    format!("{}/models", normalized)
}

fn is_obviously_non_chat_openai_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    let blocked_prefixes = [
        "text-embedding",
        "text-moderation",
        "omni-moderation",
        "whisper",
        "tts",
        "gpt-image",
        "dall-e",
        "babbage",
        "davinci",
        "curie",
        "ada",
    ];

    if blocked_prefixes.iter().any(|p| id.starts_with(p)) {
        return true;
    }
    if id.contains("instruct") {
        return true;
    }
    false
}

fn openai_model_rank(model_id: &str) -> usize {
    let id = model_id.to_lowercase();
    if id.starts_with("gpt-5") {
        0
    } else if id.starts_with("gpt-4.1") {
        1
    } else if id.starts_with("gpt-4o") {
        2
    } else if id.starts_with("gpt-4") {
        3
    } else if id.starts_with("o3") {
        4
    } else if id.starts_with("o1") {
        5
    } else {
        100
    }
}

fn is_obviously_non_generation_gemini_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    id.contains("embedding")
        || id.starts_with("aqa")
        || id.starts_with("imagen")
        || id.starts_with("veo")
}

fn gemini_model_rank(model_id: &str) -> usize {
    let id = model_id.to_lowercase();
    if id.starts_with("gemini-2.5-pro") {
        0
    } else if id.starts_with("gemini-2.5-flash") {
        1
    } else if id.starts_with("gemini-2.0-flash") {
        2
    } else if id.starts_with("gemini-2.0-pro") {
        3
    } else if id.contains("exp") {
        90
    } else if id.starts_with("gemini-1.") {
        95
    } else {
        50
    }
}

async fn send_openai_request(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    payload: &serde_json::Value,
) -> Result<(reqwest::StatusCode, String), String> {
    let mut request = client.post(url).json(payload);
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Ok((status, body))
}

fn extract_openai_message_content(res_json: &serde_json::Value) -> Result<String, String> {
    if let Some(content) = res_json["choices"][0]["message"]["content"].as_str() {
        return Ok(content.to_string());
    }

    if let Some(parts) = res_json["choices"][0]["message"]["content"].as_array() {
        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Ok(text);
        }
    }

    Err("Model response had no text content".to_string())
}

pub fn clean_json_text(text: &str) -> String {
    let text = text.trim();

    // Find the first '{' and the last '}'
    let start = text.find('{');
    let end = text.rfind('}');

    match (start, end) {
        (Some(s), Some(e)) if e > s => text[s..=e].to_string(),
        _ => text.to_string(), // Fallback to original if no braces found
    }
}

async fn call_openai_compatible(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    images: Vec<String>,
    format: ResponseFormat,
) -> Result<serde_json::Value, String> {
    let url = openai_chat_completions_url(&engine.base_url);

    let system_content = if system_prompt.contains("$USER_PROMPT") {
        system_prompt.replace("$USER_PROMPT", user_prompt)
    } else {
        system_prompt.to_string()
    };

    let mut user_content = vec![json!({ "type": "text", "text": user_prompt })];

    for b64 in images {
        user_content.push(json!({
            "type": "image_url",
            "image_url": { "url": b64 }
        }));
    }

    let base_payload = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_content },
            { "role": "user", "content": user_content }
        ]
    });
    let (status, body) = send_openai_request(client, &url, &engine.api_key, &base_payload).await?;
    if !status.is_success() {
        let body_lc = body.to_lowercase();
        if body_lc.contains("not a chat model")
            || (body_lc.contains("not supported in the v1/chat/completions endpoint")
                && body_lc.contains("model"))
        {
            return Err(format!(
                "Model '{}' is not compatible with chat completions. Choose a chat-capable model in Settings (e.g. gpt-4o, gpt-4.1, gpt-5). Raw provider error: {}",
                model, body
            ));
        }
        return Err(format!("OpenAI Error {}: {}", status, body));
    }

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let content = extract_openai_message_content(&res_json)?;

    let clean_content = clean_json_text(&content);
    match format {
        ResponseFormat::DesignOutput => {
            let parsed: DesignOutput =
                serde_json::from_str(&clean_content).map_err(|_| content.clone())?;
            Ok(serde_json::to_value(parsed).unwrap())
        }
        ResponseFormat::JsonObject => {
            serde_json::from_str(&clean_content).map_err(|_| content.clone())
        }
    }
}

async fn call_gemini(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    images: Vec<String>,
    format: ResponseFormat,
) -> Result<serde_json::Value, String> {
    let url = if engine.base_url.is_empty() {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        )
    } else {
        engine.base_url.clone()
    };

    let mut parts = vec![json!({ "text": user_prompt })];

    for b64_data_url in images {
        if let Some(b64) = b64_data_url.strip_prefix("data:image/jpeg;base64,") {
            parts.push(json!({
                "inlineData": {
                    "mimeType": "image/jpeg",
                    "data": b64
                }
            }));
        } else if let Some(b64) = b64_data_url.strip_prefix("data:image/png;base64,") {
            parts.push(json!({
                "inlineData": {
                    "mimeType": "image/png",
                    "data": b64
                }
            }));
        }
    }

    let system_content = if system_prompt.contains("$USER_PROMPT") {
        system_prompt.replace("$USER_PROMPT", user_prompt)
    } else {
        system_prompt.to_string()
    };

    let payload = json!({
        "system_instruction": {
            "parts": [ { "text": system_content } ]
        },
        "contents": [
            { "parts": parts }
        ],
        "generationConfig": {
            "responseMimeType": "application/json"
        }
    });

    let response = client
        .post(&url)
        .header("x-goog-api-key", &engine.api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            eprintln!("[LLM] Request SEND failed: {:?}", e);
            e.to_string()
        })?;

    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        return Err(format!("Gemini Error {}: {}", status, body));
    }

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        eprintln!("[LLM] JSON parse error: {}", e);
        e.to_string()
    })?;
    let text = res_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| {
            eprintln!("[LLM] No text in response. Full JSON: {}", body);
            "Gemini response had no text content".to_string()
        })?;

    let clean_text = clean_json_text(text);
    match format {
        ResponseFormat::DesignOutput => {
            let parsed: DesignOutput = serde_json::from_str(&clean_text).map_err(|e| {
                eprintln!("[LLM] DesignOutput parse FAILED: {}", e);
                eprintln!("[LLM] Raw text was: {}", text);
                text.to_string()
            })?;
            Ok(serde_json::to_value(parsed).unwrap())
        }
        ResponseFormat::JsonObject => {
            serde_json::from_str(&clean_text).map_err(|_| text.to_string())
        }
    }
}

async fn fetch_openai_models(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let url = openai_models_url(base_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI Models Error: {}", body));
    }

    let res_json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let all_models = res_json["data"]
        .as_array()
        .ok_or("Invalid response from OpenAI")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();

    let filtered = all_models
        .iter()
        .filter(|id| !is_obviously_non_chat_openai_model(id))
        .cloned()
        .collect::<Vec<_>>();

    let mut models = if filtered.is_empty() {
        all_models
    } else {
        filtered
    };

    models.sort_by(|a, b| {
        openai_model_rank(a)
            .cmp(&openai_model_rank(b))
            .then_with(|| a.cmp(b))
    });

    Ok(models)
}

async fn fetch_gemini_models(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models";
    let response = client
        .get(url)
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Gemini Models Error: {}", body));
    }

    let res_json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let all_models = res_json["models"]
        .as_array()
        .ok_or("Invalid response from Gemini")?
        .iter()
        .filter(|m| {
            m["supportedGenerationMethods"]
                .as_array()
                .map(|methods| methods.iter().any(|meth| meth == "generateContent"))
                .unwrap_or(false)
        })
        .filter_map(|m| m["name"].as_str().map(|s| s.replace("models/", "")))
        .collect::<Vec<_>>();

    let filtered = all_models
        .iter()
        .filter(|id| !is_obviously_non_generation_gemini_model(id))
        .cloned()
        .collect::<Vec<_>>();

    let mut models = if filtered.is_empty() {
        all_models
    } else {
        filtered
    };

    models.sort_by(|a, b| {
        gemini_model_rank(a)
            .cmp(&gemini_model_rank(b))
            .then_with(|| a.cmp(b))
    });

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_json_text_extracts_from_markdown() {
        let input = "```json\n{\"title\": \"Box\"}\n```";
        let result = clean_json_text(input);
        assert_eq!(result, "{\"title\": \"Box\"}");
    }

    #[test]
    fn clean_json_text_returns_original_if_no_braces() {
        let input = "no json here";
        let result = clean_json_text(input);
        assert_eq!(result, "no json here");
    }

    #[test]
    fn clean_json_text_handles_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        let result = clean_json_text(input);
        assert_eq!(result, r#"{"outer": {"inner": "value"}}"#);
    }

    #[test]
    fn clean_json_text_handles_direct_json() {
        let input = r#"{"key": "value"}"#;
        let result = clean_json_text(input);
        assert_eq!(result, r#"{"key": "value"}"#);
    }

    #[test]
    fn clean_json_text_handles_text_before_and_after() {
        let input = "Here is the result: {\"a\": 1} hope that helps";
        let result = clean_json_text(input);
        assert_eq!(result, "{\"a\": 1}");
    }

    #[test]
    fn openai_urls_normalize_root_and_models() {
        assert_eq!(
            openai_chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url("https://api.openai.com/v1/models"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_models_url_normalizes_chat_url() {
        assert_eq!(
            openai_models_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            openai_models_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/models"
        );
    }
}
