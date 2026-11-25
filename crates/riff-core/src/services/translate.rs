use super::*;
use crate::secrets::SecretName;

fn deepl_target_code(lang: &str) -> Option<&'static str> {
  Some(match lang {
    "en" => "EN",
    "ru" => "RU",
    "uk" => "UK",
    "de" => "DE",
    "fr" => "FR",
    "es" => "ES",
    "it" => "IT",
    "pt" => "PT",
    "pl" => "PL",
    "nl" => "NL",
    "ja" => "JA",
    "zh-CN" | "zh-TW" => "ZH",
    "ko" => "KO",
    "tr" => "TR",
    "sv" => "SV",
    "da" => "DA",
    "fi" => "FI",
    "cs" => "CS",
    "ro" => "RO",
    "hu" => "HU",
    "el" => "EL",
    "bg" => "BG",
    "id" => "ID",
    _ => return None,
  })
}

fn i18n_fix(lang: &str) -> Vec<(&'static str, &'static str)> {
  if lang == "ru" {
    vec![
      ("About", "О приложении"),
      ("About the artist", "Об исполнителе"),
      ("Follow", "Подписаться"),
      ("Following", "Вы подписаны"),
      ("Word", "Слово"),
      ("Line", "Строка"),
      ("Default output", "Устройство по умолчанию"),
      (
        "No local tracks in library yet.",
        "В библиотеке пока нет локальных треков.",
      ),
      ("Add files", "Добавить файлы"),
    ]
  } else {
    Vec::new()
  }
}

fn i18n_value_broken(value: Option<&Value>) -> bool {
  let Some(Value::String(s)) = value else {
    return true;
  };
  let v = s.trim();
  if v.is_empty() || v.contains('\u{FFFD}') {
    return true;
  }
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"(?i)https?://|linux\.do").expect("regex i18n"));
  re.is_match(v)
}

async fn query_official_deepl(
  client: &Http,
  text: &str,
  target: &str,
  key: &str,
) -> Result<Vec<String>, String> {
  let host = if key.ends_with(":fx") {
    "api-free.deepl.com"
  } else {
    "api.deepl.com"
  };
  let resp = client
    .post(format!("https://{host}/v2/translate"))
    .header(header::AUTHORIZATION, format!("DeepL-Auth-Key {key}"))
    .json(&json!({
      "text": text.split('\n').collect::<Vec<_>>(),
      "target_lang": target,
      "preserve_formatting": true
    }))
    .timeout(Duration::from_secs(9))
    .send()
    .await
    .map_err(|e| e.to_string())?;
  if resp.status() != 200 {
    return Err(format!("DeepL API HTTP {}", resp.status().as_u16()));
  }
  let data: Value = resp.json().await.map_err(|e| e.to_string())?;
  let arr = data
    .get("translations")
    .and_then(Value::as_array)
    .ok_or_else(|| "Invalid DeepL response".to_string())?;
  Ok(arr.iter().map(|t| value_to_string(t.get("text"))).collect())
}

async fn query_google_translate(client: &Http, text: &str, to: &str) -> Option<Vec<String>> {
  if text.is_empty() || to.is_empty() {
    return None;
  }
  let clean_to = to.split(['-', '_']).next().unwrap_or(to);
  let resp = client
    .post(format!(
      "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t",
      encode_uri_component(clean_to)
    ))
    .header(
      header::CONTENT_TYPE,
      "application/x-www-form-urlencoded;charset=UTF-8",
    )
    .header(
      header::USER_AGENT,
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0 Safari/537.36",
    )
    .body(format!("q={}", encode_uri_component(text)))
    .timeout(Duration::from_secs(6))
    .send()
    .await
    .ok()?;
  if resp.status() != 200 {
    return None;
  }
  let data: Value = resp.json().await.ok()?;
  let parts = data.get(0)?.as_array()?;
  let joined: String = parts
    .iter()
    .map(|s| s.get(0).and_then(Value::as_str).unwrap_or(""))
    .collect();
  Some(joined.split('\n').map(|s| s.trim().to_string()).collect())
}

fn clean_translations(arr: Vec<String>) -> Vec<String> {
  arr
    .into_iter()
    .map(|s| {
      s.replace("__RIFF_BRAND__", "Riff")
        .replace("__RIFF_APP__", "Riff")
    })
    .filter(|s| {
      let low = s.to_lowercase();
      !s.is_empty()
        && !s.contains("linux.do")
        && !low.contains("http://")
        && !low.contains("https://")
    })
    .collect()
}

async fn call_deepl_translate(
  client: &Http,
  text: &str,
  to: &str,
  api_key: &str,
) -> Option<Vec<String>> {
  if text.is_empty() {
    return None;
  }
  let safe = text.replace("Riff", "__RIFF_BRAND__");
  let results = match deepl_target_code(to) {
    None => query_google_translate(client, &safe, to).await,
    Some(code) => {
      let mut res = if api_key.is_empty() {
        query_google_translate(client, &safe, to).await
      } else {
        match query_official_deepl(client, &safe, code, api_key).await {
          Ok(r) => Some(r),
          Err(_) => query_google_translate(client, &safe, to).await,
        }
      };
      if res.is_none() {
        res = query_google_translate(client, &safe, to).await;
      }
      res
    }
  };
  results.map(clean_translations)
}

fn stored_deepl_key(core: &Core) -> String {
  core
    .auth
    .secrets
    .get(SecretName::DeeplKey)
    .ok()
    .flatten()
    .unwrap_or_default()
}

fn utf16_len(s: &str) -> usize {
  s.encode_utf16().count()
}

pub async fn translate(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let payload: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
  let texts = if is_truthy(payload.get("texts")) {
    payload.get("texts")
  } else {
    payload.get("lines")
  };
  let to = if is_truthy(payload.get("targetLang")) {
    payload.get("targetLang")
  } else {
    payload.get("to")
  };
  let to = if is_truthy(to) {
    value_to_string(to)
  } else {
    "en".to_string()
  };
  let key_val = if is_truthy(payload.get("apiKey")) {
    payload.get("apiKey")
  } else {
    payload.get("deeplKey")
  };
  let api_key = value_to_string(key_val).trim().to_string();

  let lines: Vec<String> = match texts.and_then(Value::as_array) {
    Some(arr) if !arr.is_empty() => arr
      .iter()
      .map(|v| {
        v.as_str()
          .map(str::to_string)
          .unwrap_or_else(|| value_to_string(Some(v)))
      })
      .collect(),
    _ => return json_response(200, &json!({ "success": true, "translations": [] })),
  };
  if !core.translation_enabled() {
    return json_response(
      200,
      &json!({ "success": false, "error": "translation disabled", "translations": [] }),
    );
  }
  let api_key = if api_key.is_empty() {
    stored_deepl_key(core)
  } else {
    api_key
  };

  let mut chunks: Vec<Vec<String>> = Vec::new();
  let mut cur: Vec<String> = Vec::new();
  let mut cur_len = 0usize;
  for line in lines {
    let l = utf16_len(&line);
    if !cur.is_empty() && cur_len + l + 1 > 1800 {
      chunks.push(std::mem::take(&mut cur));
      cur_len = 0;
    }
    cur_len += l + 1;
    cur.push(line);
  }
  if !cur.is_empty() {
    chunks.push(cur);
  }

  let client = &core.net.http_client;
  let mut all: Vec<String> = Vec::new();
  for chunk in chunks {
    let joined = chunk.join("\n");
    let mut trans = call_deepl_translate(client, &joined, &to, &api_key).await;
    if trans.as_ref().is_none_or(|t| t.len() != chunk.len()) {
      let mut per_line = Vec::with_capacity(chunk.len());
      for line in &chunk {
        if line.trim().is_empty() {
          per_line.push(String::new());
          continue;
        }
        let single = call_deepl_translate(client, line, &to, &api_key).await;
        per_line.push(
          single
            .and_then(|s| s.into_iter().next())
            .unwrap_or_else(|| line.clone()),
        );
      }
      trans = Some(per_line);
    }
    all.extend(trans.unwrap_or_default());
  }
  json_response(200, &json!({ "success": true, "translations": all }))
}

pub fn cleanup_i18n_dir() {
  let dir = paths::resolve_data_dir().join("i18n");
  let _ = fs::create_dir_all(&dir);
  if let Ok(entries) = fs::read_dir(&dir) {
    for entry in entries.flatten() {
      if let Ok(content) = fs::read_to_string(entry.path())
        && content.contains("linux.do")
      {
        let _ = fs::remove_file(entry.path());
      }
    }
  }
}

pub async fn i18n_batch(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let payload: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
  let key_val = if is_truthy(payload.get("apiKey")) {
    payload.get("apiKey")
  } else {
    payload.get("deeplKey")
  };
  let api_key = value_to_string(key_val).trim().to_string();
  let target = ["target", "targetLang", "to"]
    .iter()
    .find_map(|k| payload.get(*k).filter(|v| is_truthy(Some(v))))
    .map(|v| value_to_string(Some(v)))
    .unwrap_or_else(|| "en".to_string());
  let texts: Vec<Value> = payload
    .get("texts")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  let safe_lang: String = target
    .chars()
    .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .collect();
  let lang_file = paths::resolve_data_dir()
    .join("i18n")
    .join(format!("{safe_lang}.json"));
  let mut dict = read_json_map(&lang_file);
  let mut dirty = false;
  if safe_lang != "en" {
    let broken_keys: Vec<String> = dict
      .iter()
      .filter(|(k, v)| v.as_str() == Some(k.as_str()) || i18n_value_broken(Some(v)))
      .map(|(k, _)| k.clone())
      .collect();
    for k in broken_keys {
      dict.remove(&k);
      dirty = true;
    }
  }
  let apply_fix = |dict: &mut Map<String, Value>| {
    for (k, v) in i18n_fix(&safe_lang) {
      dict.insert(k.to_string(), json!(v));
    }
  };
  apply_fix(&mut dict);

  let mut missing: Vec<String> = Vec::new();
  for t in &texts {
    if let Some(s) = t.as_str()
      && !s.trim().is_empty()
      && !is_truthy(dict.get(s))
      && !missing.iter().any(|m| m == s)
    {
      missing.push(s.to_string());
    }
  }

  if !missing.is_empty() {
    let mut changed = false;
    if safe_lang == "en" {
      for s in &missing {
        dict.insert(s.clone(), json!(s));
        changed = true;
      }
    } else if core.translation_enabled() {
      let api_key = if api_key.is_empty() {
        stored_deepl_key(core)
      } else {
        api_key
      };
      for batch in missing.chunks(25) {
        let joined = batch.join("\n");
        if let Some(res) =
          call_deepl_translate(&core.net.http_client, &joined, &safe_lang, &api_key).await
        {
          for (src, tr) in batch.iter().zip(res.iter()) {
            let tr = tr.trim();
            if !tr.is_empty() && !i18n_value_broken(Some(&json!(tr))) {
              dict.insert(src.clone(), json!(tr));
              changed = true;
            }
          }
        }
      }
      apply_fix(&mut dict);
    }
    if changed || dirty {
      write_json_map(&lang_file, &dict);
    }
  } else if dirty {
    write_json_map(&lang_file, &dict);
  }

  let translations: Vec<Value> = texts
    .iter()
    .map(|s| {
      let looked = s.as_str().and_then(|k| dict.get(k));
      if !i18n_value_broken(looked) {
        looked.cloned().unwrap_or_else(|| s.clone())
      } else {
        s.clone()
      }
    })
    .collect();
  json_response(
    200,
    &json!({ "success": true, "translations": translations }),
  )
}
