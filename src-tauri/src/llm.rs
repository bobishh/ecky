use crate::models::{Engine, DesignOutput};
use serde_json::json;

pub async fn generate_design(engine: &Engine, prompt: &String, image_data: Option<String>) -> Result<DesignOutput, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    
    match engine.provider.as_str() {
        "openai" | "ollama" => call_openai_compatible(&client, engine, prompt, image_data).await,
        "gemini" => call_gemini(&client, engine, prompt, image_data).await,
        _ => Err(format!("Unsupported provider: {}", engine.provider)),
    }
}

pub async fn list_models(provider: &str, api_key: &str, base_url: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match provider {
        "openai" => fetch_openai_models(&client, api_key, base_url).await,
        "gemini" => fetch_gemini_models(&client, api_key).await,
        _ => Ok(vec![]),
    }
}

async fn call_openai_compatible(client: &reqwest::Client, engine: &Engine, prompt: &String, image_data: Option<String>) -> Result<DesignOutput, String> {
    let url = if engine.base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        engine.base_url.clone()
    };

    let system_content = if engine.system_prompt.contains("$USER_PROMPT") {
        engine.system_prompt.replace("$USER_PROMPT", prompt)
    } else {
        format!("{}\n\nUser intent: {}", engine.system_prompt, prompt)
    };

    let mut user_content = vec![
        json!({ "type": "text", "text": "Generate the design JSON now:" })
    ];

    if let Some(b64) = image_data {
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
    
    serde_json::from_str::<DesignOutput>(content).map_err(|e| format!("Failed to parse design JSON: {}", e))
}

async fn call_gemini(client: &reqwest::Client, engine: &Engine, prompt: &String, image_data: Option<String>) -> Result<DesignOutput, String> {
    let url = if engine.base_url.is_empty() {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", engine.model, engine.api_key)
    } else {
        engine.base_url.clone()
    };

    let system_content = if engine.system_prompt.contains("$USER_PROMPT") {
        engine.system_prompt.replace("$USER_PROMPT", prompt)
    } else {
        format!("{}\n\nUser intent: {}", engine.system_prompt, prompt)
    };

    let mut parts = vec![
        json!({ "text": format!("{}\n\nGenerate the design JSON now. Return JSON only.", system_content) })
    ];

    if let Some(b64_data_url) = image_data {
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
        "contents": [{
            "role": "user",
            "parts": parts
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
    let text = res_json["candidates"][0]["content"]["parts"][0]["text"].as_str()
        .ok_or("Gemini response had no text content")?;
    
    serde_json::from_str::<DesignOutput>(text).map_err(|e| format!("Failed to parse design JSON: {}", e))
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
