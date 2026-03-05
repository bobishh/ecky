use crate::models::{Engine, DesignOutput};
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

pub async fn generate_design(engine: &Engine, prompt: &String, images: Vec<String>) -> Result<DesignOutput, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    
    match engine.provider.as_str() {
        "openai" | "ollama" => call_openai_compatible(&client, engine, prompt, images).await,
        "gemini" => call_gemini(&client, engine, prompt, images).await,
        _ => Err(format!("Unsupported provider: {}", engine.provider)),
    }
}

pub async fn classify_intent(engine: &Engine, prompt: &str, context: Option<&str>) -> Result<IntentClassification, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
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

If intent is "question", "response" must directly answer the user's question in 1-4 concise sentences using the provided current design context when relevant.
If intent is "design", "response" must be one short routing sentence for the assistant bubble.
"#;

    let classifier_user = if let Some(context) = context.filter(|c| !c.trim().is_empty()) {
        format!("CURRENT DESIGN CONTEXT:\n{}\n\nUSER REQUEST:\n{}", context, prompt)
    } else {
        format!("USER REQUEST:\n{}", prompt)
    };

    let raw: serde_json::Value = match engine.provider.as_str() {
        "openai" | "ollama" => {
            call_openai_json_object(
                &client,
                engine,
                light_model,
                classifier_system,
                &classifier_user,
            )
            .await?
        }
        "gemini" => {
            call_gemini_json_object(
                &client,
                engine,
                light_model,
                classifier_system,
                &classifier_user,
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

pub async fn list_models(provider: &str, api_key: &str, base_url: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match provider {
        "openai" => fetch_openai_models(&client, api_key, base_url).await,
        "gemini" => fetch_gemini_models(&client, api_key).await,
        _ => Ok(vec![]),
    }
}

pub fn clean_json_text(text: &str) -> String {
    let text = text.trim();
    
    // Find the first '{' and the last '}'
    let start = text.find('{');
    let end = text.rfind('}');

    match (start, end) {
        (Some(s), Some(e)) if e > s => {
            text[s..=e].to_string()
        },
        _ => text.to_string(), // Fallback to original if no braces found
    }
}

async fn call_openai_compatible(client: &reqwest::Client, engine: &Engine, prompt: &String, images: Vec<String>) -> Result<DesignOutput, String> {
    let url = if engine.base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        engine.base_url.clone()
    };

    let system_content = if engine.system_prompt.contains("$USER_PROMPT") {
        engine.system_prompt.replace("$USER_PROMPT", prompt)
    } else {
        engine.system_prompt.clone()
    };

    let mut user_content = vec![
        json!({ "type": "text", "text": prompt })
    ];

    for b64 in images {
        user_content.push(json!({
            "type": "image_url",
            "image_url": { "url": b64 }
        }));
    }

    let payload = json!({
        "model": engine.model,
        "messages": [
            { "role": "system", "content": system_content },
            { "role": "user", "content": user_content }
        ],
        "response_format": { "type": "json_object" }
    });

    let mut request = client.post(&url).json(&payload);
    if !engine.api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", engine.api_key));
    }

    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("OpenAI Error {}: {}", status, body));
    }

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let content = res_json["choices"][0]["message"]["content"].as_str()
        .ok_or("Model response had no text content")?;
    
    let clean_content = clean_json_text(content);
    serde_json::from_str::<DesignOutput>(&clean_content).map_err(|_| content.to_string())
}

async fn call_openai_json_object(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str
) -> Result<serde_json::Value, String> {
    let url = if engine.base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        engine.base_url.clone()
    };

    let payload = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "response_format": { "type": "json_object" }
    });

    let mut request = client.post(&url).json(&payload);
    if !engine.api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", engine.api_key));
    }

    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("OpenAI Error {}: {}", status, body));
    }

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let content = res_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("Model response had no text content")?;
    let clean_content = clean_json_text(content);
    serde_json::from_str::<serde_json::Value>(&clean_content).map_err(|_| content.to_string())
}

async fn call_gemini(client: &reqwest::Client, engine: &Engine, prompt: &String, images: Vec<String>) -> Result<DesignOutput, String> {
    let url = if engine.base_url.is_empty() {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", engine.model, engine.api_key)
    } else {
        engine.base_url.clone()
    };

    let system_content = if engine.system_prompt.contains("$USER_PROMPT") {
        engine.system_prompt.replace("$USER_PROMPT", prompt)
    } else {
        engine.system_prompt.clone()
    };

    let mut parts = vec![
        json!({ "text": prompt })
    ];

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

    let payload = json!({
        "systemInstruction": {
            "parts": [{ "text": system_content }]
        },
        "contents": [{
            "role": "user",
            "parts": parts
        }],
        "generationConfig": { "responseMimeType": "application/json" }
    });

    eprintln!("[GEMINI DEBUG] URL: {}", url.split('?').next().unwrap_or(&url));
    eprintln!("[GEMINI DEBUG] Model: {}", engine.model);
    eprintln!("[GEMINI DEBUG] System prompt length: {} chars", system_content.len());
    eprintln!("[GEMINI DEBUG] User prompt length: {} chars", prompt.len());
    eprintln!("[GEMINI DEBUG] Has image: {}", parts.len() > 1);
    eprintln!("[GEMINI DEBUG] Payload size: {} bytes", serde_json::to_string(&payload).unwrap_or_default().len());
    eprintln!("[GEMINI DEBUG] User prompt preview: {:.200}", prompt);

    let response = client.post(&url).json(&payload).send().await.map_err(|e| {
        eprintln!("[GEMINI DEBUG] Request SEND failed: {:?}", e);
        e.to_string()
    })?;
    let status = response.status();
    eprintln!("[GEMINI DEBUG] Response status: {}", status);
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        eprintln!("[GEMINI DEBUG] Error body: {}", body);
        return Err(format!("Gemini Error {}: {}", status, body));
    }

    eprintln!("[GEMINI DEBUG] Response body length: {} chars", body.len());
    eprintln!("[GEMINI DEBUG] Response preview: {:.500}", body);

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        eprintln!("[GEMINI DEBUG] JSON parse error: {}", e);
        e.to_string()
    })?;
    let text = res_json["candidates"][0]["content"]["parts"][0]["text"].as_str()
        .ok_or_else(|| {
            eprintln!("[GEMINI DEBUG] No text in response. Full JSON: {}", body);
            "Gemini response had no text content".to_string()
        })?;
    
    eprintln!("[GEMINI DEBUG] Extracted text length: {} chars", text.len());
    let clean_text = clean_json_text(text);
    eprintln!("[GEMINI DEBUG] Clean JSON preview: {:.300}", clean_text);
    
    serde_json::from_str::<DesignOutput>(&clean_text).map_err(|e| {
        eprintln!("[GEMINI DEBUG] DesignOutput parse FAILED: {}", e);
        eprintln!("[GEMINI DEBUG] Raw text was: {}", text);
        text.to_string()
    })
}

async fn call_gemini_json_object(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str
) -> Result<serde_json::Value, String> {
    let url = if engine.base_url.is_empty() {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, engine.api_key
        )
    } else {
        engine.base_url.clone()
    };

    let payload = json!({
        "systemInstruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": [{
            "role": "user",
            "parts": [{ "text": user_prompt }]
        }],
        "generationConfig": { "responseMimeType": "application/json" }
    });

    let response = client.post(&url).json(&payload).send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("Gemini Error {}: {}", status, body));
    }

    let res_json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let text = res_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or("Gemini response had no text content")?;
    let clean_text = clean_json_text(text);
    serde_json::from_str::<serde_json::Value>(&clean_text).map_err(|_| text.to_string())
}

async fn fetch_openai_models(client: &reqwest::Client, api_key: &str, base_url: &str) -> Result<Vec<String>, String> {
    let url = if base_url.is_empty() {
        "https://api.openai.com/v1/models".to_string()
    } else {
        format!("{}/models", base_url.trim_end_matches("/chat/completions"))
    };

    let response = client.get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send().await.map_err(|e| e.to_string())?;
    
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI Models Error: {}", body));
    }

    let res_json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let models = res_json["data"].as_array()
        .ok_or("Invalid response from OpenAI")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();
    Ok(models)
}

async fn fetch_gemini_models(client: &reqwest::Client, api_key: &str) -> Result<Vec<String>, String> {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={}", api_key);
    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Gemini Models Error: {}", body));
    }

    let res_json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let models = res_json["models"].as_array()
        .ok_or("Invalid response from Gemini")?
        .iter()
        .filter(|m| {
            m["supportedGenerationMethods"].as_array()
                .map(|methods| methods.iter().any(|meth| meth == "generateContent"))
                .unwrap_or(false)
        })
        .filter_map(|m| m["name"].as_str().map(|s| s.replace("models/", "")))
        .collect();
    Ok(models)
}
