use crate::models::{DesignOutput, Engine, UsageSegment, UsageSummary};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntentClassification {
    pub intent: String, // "question" | "design"
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub final_response: Option<String>,
}

pub enum ResponseFormat {
    DesignOutput,
    JsonObject,
}

#[derive(Debug, Clone)]
pub struct LlmOutcome<T> {
    pub data: T,
    pub usage: Option<UsageSummary>,
}

pub async fn generate_design(
    engine: &Engine,
    prompt: &str,
    images: Vec<String>,
) -> Result<LlmOutcome<DesignOutput>, String> {
    if !engine.enabled {
        return Err(format!(
            "Engine \"{}\" is disabled. Enable it in Settings → Agents before making API calls.",
            engine.name
        ));
    }
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
                "generate",
                ResponseFormat::DesignOutput,
            )
            .await?;
            let data = serde_json::from_value(res.data).map_err(|e| e.to_string())?;
            Ok(LlmOutcome {
                data,
                usage: res.usage,
            })
        }
        "gemini" => {
            let res = call_gemini(
                &client,
                engine,
                engine.model.as_str(),
                &engine.system_prompt,
                prompt,
                images,
                "generate",
                ResponseFormat::DesignOutput,
            )
            .await?;
            let data = serde_json::from_value(res.data).map_err(|e| e.to_string())?;
            Ok(LlmOutcome {
                data,
                usage: res.usage,
            })
        }
        _ => Err(format!("Unsupported provider: {}", engine.provider)),
    }
}

pub async fn classify_intent(
    engine: &Engine,
    prompt: &str,
    context: Option<&str>,
    images: Vec<String>,
) -> Result<LlmOutcome<IntentClassification>, String> {
    if !engine.enabled {
        return Err(format!(
            "Engine \"{}\" is disabled. Enable it in Settings → Agents before making API calls.",
            engine.name
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let classifier_model = select_classifier_model(engine, !images.is_empty());

    let classifier_system = r#"Return ONLY JSON with fields:
1) "intent": "question" or "design"
2) "confidence": number in [0, 1]
3) "response": short routing/bubble text
4) "final_response": either null or a final user-facing answer

Choose "question" when user asks to explain, inspect, compare, clarify, or asks "why/how/what" about existing design/code.
Choose "design" when user asks to create/change/add/remove geometry, parameters, dimensions, connectors, or regenerate output.
If the user explicitly says to only answer and not generate anything, such as "answer only", "just answer", "do not generate", "только ответь", or "без генерации", always choose "question".
If the user is asking whether something can be changed, why something behaves a certain way, what a parameter/feature does, or how to approach a change, prefer "question" if you can answer from the current context. Do NOT choose "design" just because the request mentions CAD verbs like move, resize, increase, decrease, or add.
Only choose "design" when the user is clearly instructing you to actually perform the geometry change now.
If the recent dialogue ends with an assistant clarification question and the new user message looks like a short direct answer to that question, prefer "design" so the pending change can continue.
Do not ask the same clarification question again unless the new answer is still genuinely unusable.

If you can fully answer immediately from the provided context, set "intent" to "question" and put that final answer in "final_response".
Use "response" for the short bubble/routing text. It can be brief even when "final_response" contains the full answer.
If more work is still needed after classification, "final_response" must be null.
If intent is "design", "response" must be one short routing sentence for the assistant bubble and "final_response" must be null.
"#;

    let classifier_user = if let Some(context) = context.filter(|c| !c.trim().is_empty()) {
        format!(
            "CURRENT DESIGN CONTEXT:\n{}\n\nUSER REQUEST:\n{}",
            context, prompt
        )
    } else {
        format!("USER REQUEST:\n{}", prompt)
    };

    let raw = match engine.provider.as_str() {
        "openai" | "ollama" => {
            call_openai_compatible(
                &client,
                engine,
                classifier_model,
                classifier_system,
                &classifier_user,
                images,
                "classify",
                ResponseFormat::JsonObject,
            )
            .await?
        }
        "gemini" => {
            call_gemini(
                &client,
                engine,
                classifier_model,
                classifier_system,
                &classifier_user,
                images,
                "classify",
                ResponseFormat::JsonObject,
            )
            .await?
        }
        _ => return Err(format!("Unsupported provider: {}", engine.provider)),
    };

    let mut parsed: IntentClassification =
        serde_json::from_value(raw.data).map_err(|e| format!("Intent parse error: {}", e))?;
    parsed.intent = parsed.intent.to_lowercase();
    if parsed.intent != "question" && parsed.intent != "design" {
        parsed.intent = "design".to_string();
    }
    if !(0.0..=1.0).contains(&parsed.confidence) {
        parsed.confidence = 0.5;
    }
    parsed.final_response = parsed
        .final_response
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if parsed.final_response.is_some() {
        parsed.intent = "question".to_string();
    }
    if parsed.response.trim().is_empty() {
        parsed.response = if parsed.intent == "question" {
            "Thinking not deep enough. Treating this as a question.".to_string()
        } else {
            "Intent looks like a design change request.".to_string()
        };
    }
    Ok(LlmOutcome {
        data: parsed,
        usage: raw.usage,
    })
}

fn select_classifier_model(engine: &Engine, has_images: bool) -> &str {
    if has_images || engine.light_model.trim().is_empty() {
        engine.model.as_str()
    } else {
        engine.light_model.as_str()
    }
}

/// Fetch models for an auto-agent CLI tool.
/// Tries to read the relevant env var (GEMINI_API_KEY, ANTHROPIC_API_KEY, OPENAI_API_KEY)
/// and hit the live API. Falls back to a curated static list when no key is available.
pub async fn list_agent_models(cmd: &str) -> Result<crate::contracts::AgentModelList, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let cmd_name = std::path::Path::new(cmd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(cmd)
        .to_lowercase();

    if cmd_name.contains("gemini") {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .or_else(|_| std::env::var("GOOGLE_GEMINI_API_KEY"))
            .unwrap_or_default();
        if !api_key.is_empty() {
            if let Ok(models) = fetch_gemini_models(&client, &api_key).await {
                if !models.is_empty() {
                    return Ok(crate::contracts::AgentModelList {
                        models,
                        is_live: true,
                    });
                }
            }
        }
        Ok(crate::contracts::AgentModelList {
            models: vec![
                "gemini-3.1-pro-preview".to_string(),
                "gemini-2.5-pro-preview-05-06".to_string(),
                "gemini-2.5-flash-preview-04-17".to_string(),
                "gemini-2.5-flash".to_string(),
                "gemini-2.0-flash".to_string(),
                "gemini-2.0-flash-lite".to_string(),
            ],
            is_live: false,
        })
    } else if cmd_name.contains("claude") {
        let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        if !api_key.is_empty() {
            if let Ok(models) = fetch_anthropic_models(&client, &api_key).await {
                if !models.is_empty() {
                    return Ok(crate::contracts::AgentModelList {
                        models,
                        is_live: true,
                    });
                }
            }
        }
        Ok(crate::contracts::AgentModelList {
            models: vec![
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-haiku-4-5-20251001".to_string(),
            ],
            is_live: false,
        })
    } else if cmd_name.contains("codex") || cmd_name.contains("openai") {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        if !api_key.is_empty() {
            if let Ok(models) = fetch_openai_models(&client, &api_key, "").await {
                if !models.is_empty() {
                    return Ok(crate::contracts::AgentModelList {
                        models,
                        is_live: true,
                    });
                }
            }
        }
        Ok(crate::contracts::AgentModelList {
            models: vec![
                "o4-mini".to_string(),
                "o3".to_string(),
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
            ],
            is_live: false,
        })
    } else {
        Ok(crate::contracts::AgentModelList {
            models: vec![],
            is_live: false,
        })
    }
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
        return format!(
            "{}/chat/completions",
            normalized.trim_end_matches("/responses")
        );
    }
    if normalized.ends_with("/models") {
        return format!(
            "{}/chat/completions",
            normalized.trim_end_matches("/models")
        );
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
        return format!(
            "{}/models",
            normalized.trim_end_matches("/chat/completions")
        );
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

fn estimate_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
) -> Option<f64> {
    #[derive(Clone, Copy)]
    struct Pricing {
        input_per_million: f64,
        output_per_million: f64,
        cached_input_per_million: Option<f64>,
    }

    fn openai_pricing(model: &str) -> Option<Pricing> {
        let id = model.to_ascii_lowercase();
        if id.starts_with("gpt-5-mini") {
            return Some(Pricing {
                input_per_million: 0.25,
                output_per_million: 2.0,
                cached_input_per_million: Some(0.025),
            });
        }
        if id.starts_with("gpt-5-nano") {
            return Some(Pricing {
                input_per_million: 0.05,
                output_per_million: 0.4,
                cached_input_per_million: Some(0.005),
            });
        }
        if id.starts_with("gpt-5") {
            return Some(Pricing {
                input_per_million: 1.25,
                output_per_million: 10.0,
                cached_input_per_million: Some(0.125),
            });
        }
        if id.starts_with("gpt-4.1-mini") {
            return Some(Pricing {
                input_per_million: 0.4,
                output_per_million: 1.6,
                cached_input_per_million: Some(0.1),
            });
        }
        if id.starts_with("gpt-4.1-nano") {
            return Some(Pricing {
                input_per_million: 0.1,
                output_per_million: 0.4,
                cached_input_per_million: Some(0.025),
            });
        }
        if id.starts_with("gpt-4.1") {
            return Some(Pricing {
                input_per_million: 2.0,
                output_per_million: 8.0,
                cached_input_per_million: Some(0.5),
            });
        }
        if id.starts_with("gpt-4o-mini") {
            return Some(Pricing {
                input_per_million: 0.15,
                output_per_million: 0.6,
                cached_input_per_million: Some(0.075),
            });
        }
        if id.starts_with("gpt-4o") {
            return Some(Pricing {
                input_per_million: 2.5,
                output_per_million: 10.0,
                cached_input_per_million: Some(1.25),
            });
        }
        None
    }

    fn gemini_pricing(model: &str, input_tokens: u64) -> Option<Pricing> {
        let id = model.to_ascii_lowercase();
        if id.starts_with("gemini-2.5-pro") {
            let high_context = input_tokens > 200_000;
            return Some(Pricing {
                input_per_million: if high_context { 2.5 } else { 1.25 },
                output_per_million: if high_context { 15.0 } else { 10.0 },
                cached_input_per_million: Some(if high_context { 0.625 } else { 0.3125 }),
            });
        }
        if id.starts_with("gemini-2.5-flash-lite") {
            return Some(Pricing {
                input_per_million: 0.1,
                output_per_million: 0.4,
                cached_input_per_million: Some(0.025),
            });
        }
        if id.starts_with("gemini-2.5-flash") {
            return Some(Pricing {
                input_per_million: 0.3,
                output_per_million: 2.5,
                cached_input_per_million: Some(0.075),
            });
        }
        if id.starts_with("gemini-2.0-flash-lite") {
            return Some(Pricing {
                input_per_million: 0.075,
                output_per_million: 0.3,
                cached_input_per_million: Some(0.01875),
            });
        }
        if id.starts_with("gemini-2.0-flash") {
            return Some(Pricing {
                input_per_million: 0.1,
                output_per_million: 0.4,
                cached_input_per_million: Some(0.025),
            });
        }
        None
    }

    let pricing = match provider {
        "openai" => openai_pricing(model),
        "gemini" => gemini_pricing(model, input_tokens),
        _ => None,
    }?;

    let effective_input = input_tokens.saturating_sub(cached_input_tokens);
    let input_cost = (effective_input as f64 / 1_000_000.0) * pricing.input_per_million;
    let cached_input_cost = (cached_input_tokens as f64 / 1_000_000.0)
        * pricing
            .cached_input_per_million
            .unwrap_or(pricing.input_per_million);
    let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_per_million;
    Some(input_cost + cached_input_cost + output_cost)
}

#[allow(clippy::too_many_arguments)]
fn usage_segment(
    stage: &str,
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cached_input_tokens: u64,
    reasoning_tokens: u64,
) -> UsageSummary {
    let estimated_cost_usd = estimate_cost_usd(
        provider,
        model,
        input_tokens,
        output_tokens,
        cached_input_tokens,
    );
    UsageSummary::from_segment(UsageSegment {
        stage: stage.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        input_tokens,
        output_tokens,
        total_tokens,
        cached_input_tokens,
        reasoning_tokens,
        estimated_cost_usd,
    })
}

fn extract_openai_usage(
    stage: &str,
    provider: &str,
    model: &str,
    res_json: &serde_json::Value,
) -> Option<UsageSummary> {
    let usage = res_json.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);
    let cached_input_tokens = usage
        .get("prompt_tokens_details")
        .and_then(|v| v.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(|v| v.get("reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if input_tokens == 0 && output_tokens == 0 && total_tokens == 0 {
        return None;
    }

    Some(usage_segment(
        stage,
        provider,
        model,
        input_tokens,
        output_tokens,
        total_tokens,
        cached_input_tokens,
        reasoning_tokens,
    ))
}

fn extract_gemini_usage(
    stage: &str,
    model: &str,
    res_json: &serde_json::Value,
) -> Option<UsageSummary> {
    let usage = res_json.get("usageMetadata")?;
    let input_tokens = usage
        .get("promptTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("candidatesTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("totalTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);
    let cached_input_tokens = usage
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("thoughtsTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if input_tokens == 0 && output_tokens == 0 && total_tokens == 0 {
        return None;
    }

    Some(usage_segment(
        stage,
        "gemini",
        model,
        input_tokens,
        output_tokens,
        total_tokens,
        cached_input_tokens,
        reasoning_tokens,
    ))
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

#[allow(clippy::too_many_arguments)]
async fn call_openai_compatible(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    images: Vec<String>,
    stage: &str,
    format: ResponseFormat,
) -> Result<LlmOutcome<serde_json::Value>, String> {
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
    let usage = extract_openai_usage(stage, &engine.provider, model, &res_json);

    let clean_content = clean_json_text(&content);
    match format {
        ResponseFormat::DesignOutput => {
            let parsed: DesignOutput =
                serde_json::from_str(&clean_content).map_err(|_| content.clone())?;
            Ok(LlmOutcome {
                data: serde_json::to_value(parsed).unwrap(),
                usage,
            })
        }
        ResponseFormat::JsonObject => {
            let data = serde_json::from_str(&clean_content).map_err(|_| content.clone())?;
            Ok(LlmOutcome { data, usage })
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn call_gemini(
    client: &reqwest::Client,
    engine: &Engine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    images: Vec<String>,
    stage: &str,
    format: ResponseFormat,
) -> Result<LlmOutcome<serde_json::Value>, String> {
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
    let usage = extract_gemini_usage(stage, model, &res_json);
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
            Ok(LlmOutcome {
                data: serde_json::to_value(parsed).unwrap(),
                usage,
            })
        }
        ResponseFormat::JsonObject => {
            let data = serde_json::from_str(&clean_text).map_err(|_| text.to_string())?;
            Ok(LlmOutcome { data, usage })
        }
    }
}

async fn fetch_anthropic_models(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let response = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic Models Error: {}", body));
    }

    let res_json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let mut models = res_json["data"]
        .as_array()
        .ok_or("Invalid response from Anthropic")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .filter(|id| id.starts_with("claude-"))
        .collect::<Vec<_>>();

    // Newest first (lexicographic desc works well for claude-X-Y versioning)
    models.sort_by(|a, b| b.cmp(a));
    Ok(models)
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

    #[test]
    fn extracts_openai_usage_and_cost() {
        let payload = json!({
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 200,
                "total_tokens": 1200,
                "prompt_tokens_details": { "cached_tokens": 100 },
                "completion_tokens_details": { "reasoning_tokens": 20 }
            }
        });

        let usage = extract_openai_usage("generate", "openai", "gpt-4o-mini", &payload)
            .expect("usage should parse");

        assert_eq!(usage.total_tokens, 1200);
        assert_eq!(usage.cached_input_tokens, 100);
        assert_eq!(usage.reasoning_tokens, 20);
        assert_eq!(usage.segments.len(), 1);
        assert!(usage.estimated_cost_usd.unwrap_or_default() > 0.0);
    }

    #[test]
    fn extracts_gemini_usage_and_cost() {
        let payload = json!({
            "usageMetadata": {
                "promptTokenCount": 500,
                "candidatesTokenCount": 125,
                "totalTokenCount": 625,
                "cachedContentTokenCount": 50,
                "thoughtsTokenCount": 10
            }
        });

        let usage = extract_gemini_usage("classify", "gemini-2.0-flash", &payload)
            .expect("usage should parse");

        assert_eq!(usage.input_tokens, 500);
        assert_eq!(usage.output_tokens, 125);
        assert_eq!(usage.cached_input_tokens, 50);
        assert_eq!(usage.reasoning_tokens, 10);
        assert_eq!(usage.segments[0].stage, "classify");
        assert!(usage.estimated_cost_usd.unwrap_or_default() > 0.0);
    }

    #[test]
    fn classifier_uses_heavy_model_when_images_are_present() {
        let engine = Engine {
            id: "test".to_string(),
            name: "Test".to_string(),
            provider: "openai".to_string(),
            api_key: "key".to_string(),
            model: "gpt-4o".to_string(),
            light_model: "gpt-4.1-nano".to_string(),
            base_url: String::new(),
            system_prompt: String::new(),
            enabled: true,
        };

        assert_eq!(select_classifier_model(&engine, true), "gpt-4o");
        assert_eq!(select_classifier_model(&engine, false), "gpt-4.1-nano");
    }

    #[test]
    fn classifier_falls_back_to_heavy_model_when_light_model_is_empty() {
        let engine = Engine {
            id: "test".to_string(),
            name: "Test".to_string(),
            provider: "openai".to_string(),
            api_key: "key".to_string(),
            model: "gpt-4o".to_string(),
            light_model: String::new(),
            base_url: String::new(),
            system_prompt: String::new(),
            enabled: true,
        };

        assert_eq!(select_classifier_model(&engine, false), "gpt-4o");
    }
}
