//! Services : recherche, flux, paroles, imports et outils externes.

use crate::core::{Core, MAX_BODY_BYTES};
use crate::library::find_cached_file;
use crate::models::normalize_track_obj;
use crate::net::{self, Http};
use crate::privacy::clean_url;
use crate::reply::{CoreResult, Method, Params, json_response, text_response};
use crate::util::{
  clean_track_id, encode_uri_component, fetch_itunes_json, get_track_normalized_key,
  http_get_json_reached, is_spam_or_unofficial, is_text_explicit, is_truthy, lyrics_key,
  normalize_number_value, now_millis, parse_duration_seconds, read_json_map, sha1_hex,
  strip_youtube_topic, upgrade_artwork_url, value_to_string, write_json_map,
};
use crate::{config, logging, paths};
use regex::Regex;
use reqwest::header;
use serde_json::{Map, Value, json};
use sha1::Digest;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use url::Url;

pub mod download;
pub mod google;
pub mod imports;
pub mod lastfm;
pub mod lyrics;
pub mod playback;
pub mod playlist;
pub mod search;
pub mod soundcloud;
pub mod spotify;
pub mod stream;
pub mod tools;
pub mod translate;
pub mod youtube;
pub mod ytdlp;
