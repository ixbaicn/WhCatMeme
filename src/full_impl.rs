use std::{
  collections::HashMap,
  fs,
  panic::{self, AssertUnwindSafe},
  path::PathBuf,
  time::{SystemTime, UNIX_EPOCH},
};

use meme_generator::meme::{Image, MemeInfo, MemeOption, OptionValue, ParserFlags};
use meme_generator::{
  get_meme, get_meme_keys, get_memes, read_config_file, resources, search_memes, tools, MEME_HOME,
  VERSION,
};
use napi::bindgen_prelude::{Buffer, Error, Result, Status};
use napi_derive::napi;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

const DEFAULT_MAX_TEXT_LENGTH: usize = 512;
const MAX_BLOB_SIZE: usize = 20 * 1024 * 1024;
const MAX_IMAGE_COUNT: usize = 32;

const CODE_IMAGE_COUNT_MISMATCH: &str = "IMAGE_COUNT_MISMATCH";
const CODE_TEXT_COUNT_MISMATCH: &str = "TEXT_COUNT_MISMATCH";
const CODE_ASSET_MISSING: &str = "ASSET_MISSING";
const CODE_RESOURCE_MISSING: &str = "RESOURCE_MISSING";
const CODE_INVALID_OPTION: &str = "INVALID_OPTION";
const CODE_MEME_NOT_FOUND: &str = "MEME_NOT_FOUND";
const CODE_MEME_DISABLED: &str = "MEME_DISABLED";
const CODE_INVALID_PAYLOAD: &str = "INVALID_PAYLOAD";
const CODE_RANDOM_GENERATION_FAILED: &str = "RANDOM_GENERATION_FAILED";
const CODE_INTERNAL_PANIC: &str = "INTERNAL_PANIC";

#[derive(Clone)]
struct StateStore {
  db_path: PathBuf,
}

impl StateStore {
  fn new(db_path: Option<String>) -> Result<Self> {
    let default_db = MEME_HOME.join("whcatmeme.sqlite");
    let db_path = db_path.map(PathBuf::from).unwrap_or(default_db);
    if let Some(parent) = db_path.parent() {
      fs::create_dir_all(parent).map_err(io_err)?;
    }
    let store = Self { db_path };
    store.init_schema()?;
    Ok(store)
  }

  fn init_schema(&self) -> Result<()> {
    let conn = self.open()?;
    conn
      .execute_batch(
        "
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS meme_state (
          meme_key TEXT PRIMARY KEY,
          enabled INTEGER NOT NULL,
          updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
      ",
      )
      .map_err(sql_err)
  }

  fn open(&self) -> Result<Connection> {
    Connection::open(&self.db_path).map_err(sql_err)
  }

  fn set_enabled(&self, key: &str, enabled: bool) -> Result<()> {
    let conn = self.open()?;
    conn
      .execute(
        "
        INSERT INTO meme_state (meme_key, enabled, updated_at)
        VALUES (?1, ?2, CURRENT_TIMESTAMP)
        ON CONFLICT(meme_key) DO UPDATE SET
          enabled = excluded.enabled,
          updated_at = CURRENT_TIMESTAMP
      ",
        params![key, if enabled { 1 } else { 0 }],
      )
      .map(|_| ())
      .map_err(sql_err)
  }

  fn is_enabled(&self, key: &str) -> Result<bool> {
    let conn = self.open()?;
    let mut stmt = conn
      .prepare("SELECT enabled FROM meme_state WHERE meme_key = ?1")
      .map_err(sql_err)?;
    let value: Option<i64> = stmt.query_row(params![key], |row| row.get(0)).ok();
    Ok(value.unwrap_or(1) != 0)
  }

  fn list_states(&self) -> Result<Vec<MemeState>> {
    let conn = self.open()?;
    let mut stmt = conn
      .prepare("SELECT meme_key, enabled FROM meme_state ORDER BY meme_key")
      .map_err(sql_err)?;
    let rows = stmt
      .query_map([], |row| {
        Ok(MemeState {
          key: row.get(0)?,
          enabled: row.get::<_, i64>(1)? != 0,
        })
      })
      .map_err(sql_err)?;
    let mut out = Vec::new();
    for row in rows {
      out.push(row.map_err(sql_err)?);
    }
    Ok(out)
  }

  fn disabled_keys(&self) -> Result<Vec<String>> {
    let conn = self.open()?;
    let mut stmt = conn
      .prepare("SELECT meme_key FROM meme_state WHERE enabled = 0 ORDER BY meme_key")
      .map_err(sql_err)?;
    let rows = stmt
      .query_map([], |row| row.get::<_, String>(0))
      .map_err(sql_err)?;
    let mut out = Vec::new();
    for row in rows {
      out.push(row.map_err(sql_err)?);
    }
    Ok(out)
  }
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct MemeState {
  pub key: String,
  pub enabled: bool,
}

#[napi(object)]
pub struct ParserFlagsDto {
  pub short: bool,
  pub long: bool,
  pub short_aliases: Vec<String>,
  pub long_aliases: Vec<String>,
}

#[napi(object)]
pub struct MemeOptionDto {
  pub option_type: String,
  pub name: String,
  pub description: Option<String>,
  pub parser_flags: ParserFlagsDto,
  pub default_value: Option<Value>,
  pub choices: Option<Vec<String>>,
  pub minimum: Option<f64>,
  pub maximum: Option<f64>,
}

#[napi(object)]
pub struct MemeParamsDto {
  pub min_images: u32,
  pub max_images: u32,
  pub min_texts: u32,
  pub max_texts: u32,
  pub default_texts: Vec<String>,
  pub options: Vec<MemeOptionDto>,
}

#[napi(object)]
pub struct MemeShortcutDto {
  pub pattern: String,
  pub humanized: Option<String>,
  pub names: Vec<String>,
  pub texts: Vec<String>,
  pub options: Value,
}

#[napi(object)]
pub struct MemeInfoDto {
  pub key: String,
  pub params: MemeParamsDto,
  pub keywords: Vec<String>,
  pub shortcuts: Vec<MemeShortcutDto>,
  pub tags: Vec<String>,
  pub date_created: String,
  pub date_modified: String,
  pub enabled: bool,
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct InitOptions {
  pub db_path: Option<String>,
  pub max_text_length: Option<u32>,
}

#[napi(object)]
pub struct InputImagePayload {
  pub name: Option<String>,
  pub data: Buffer,
}

#[napi(object)]
pub struct GenerateMemePayload {
  pub key: String,
  pub images: Option<Vec<InputImagePayload>>,
  pub texts: Option<Vec<String>>,
  pub options: Option<Value>,
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct ValidationIssue {
  pub code: String,
  pub field: String,
  pub message: String,
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct GenerateValidationResult {
  pub ok: bool,
  pub issues: Vec<ValidationIssue>,
  pub required_min_images: u32,
  pub required_max_images: u32,
  pub required_min_texts: u32,
  pub required_max_texts: u32,
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct ResourceStatus {
  pub key: String,
  pub enabled: bool,
  pub available: bool,
  pub code: Option<String>,
  pub message: Option<String>,
}

#[napi(object)]
pub struct GenerateMemeResult {
  pub key: String,
  pub buffer: Buffer,
  pub mime: String,
  pub used_images: u32,
  pub used_texts: u32,
}

#[napi(object)]
pub struct RandomGenerateFilter {
  pub require_images: Option<bool>,
  pub min_texts: Option<u32>,
  pub max_texts: Option<u32>,
  pub exclude_keys: Option<Vec<String>>,
  pub include_disabled: Option<bool>,
}

#[napi(object)]
pub struct GenerateRandomPayload {
  pub filters: Option<RandomGenerateFilter>,
  pub images: Option<Vec<InputImagePayload>>,
  pub texts: Option<Vec<String>>,
  pub options: Option<Value>,
}

#[napi]
pub struct MemeGenerator {
  state_store: StateStore,
  max_text_length: usize,
}

#[napi]
impl MemeGenerator {
  #[napi(constructor)]
  pub fn new(options: Option<InitOptions>) -> Result<Self> {
    let options = options.unwrap_or(InitOptions {
      db_path: None,
      max_text_length: None,
    });
    let max_text_length = options
      .max_text_length
      .unwrap_or(DEFAULT_MAX_TEXT_LENGTH as u32) as usize;
    if max_text_length == 0 || max_text_length > 4096 {
      return Err(Error::new(
        Status::InvalidArg,
        "maxTextLength must be in [1, 4096]".to_string(),
      ));
    }
    let state_store = StateStore::new(options.db_path)?;
    Ok(Self {
      state_store,
      max_text_length,
    })
  }

  #[napi]
  pub fn version(&self) -> String {
    VERSION.to_string()
  }

  #[napi]
  pub fn meme_home(&self) -> String {
    MEME_HOME.to_string_lossy().to_string()
  }

  #[napi]
  pub fn state_db_path(&self) -> String {
    self.state_store.db_path.to_string_lossy().to_string()
  }

  #[napi]
  pub fn read_config_file(&self) -> String {
    read_config_file()
  }

  #[napi]
  pub fn get_meme_keys(&self, include_disabled: Option<bool>) -> Result<Vec<String>> {
    let include_disabled = include_disabled.unwrap_or(false);
    let keys = get_meme_keys()
      .into_iter()
      .map(ToOwned::to_owned)
      .collect::<Vec<_>>();
    if include_disabled {
      return Ok(keys);
    }
    filter_enabled_keys(&self.state_store, keys)
  }

  #[napi]
  pub fn get_meme_info(&self, key: String) -> Result<Option<MemeInfoDto>> {
    let key = sanitize_key(&key)?;
    if !self.state_store.is_enabled(&key)? {
      return Ok(None);
    }
    Ok(get_meme(&key).map(|meme| meme_info_to_dto(&meme.info(), true)))
  }

  #[napi]
  pub fn get_memes_info(&self, include_disabled: Option<bool>) -> Result<Vec<MemeInfoDto>> {
    let include_disabled = include_disabled.unwrap_or(false);
    let mut out = Vec::new();
    for meme in get_memes() {
      let key = meme.key();
      let enabled = self.state_store.is_enabled(&key)?;
      if !include_disabled && !enabled {
        continue;
      }
      out.push(meme_info_to_dto(&meme.info(), enabled));
    }
    Ok(out)
  }

  #[napi]
  pub fn search_memes(
    &self,
    query: String,
    include_tags: Option<bool>,
    include_disabled: Option<bool>,
  ) -> Result<Vec<String>> {
    let query = sanitize_query(&query)?;
    let keys = search_memes(&query, include_tags.unwrap_or(true));
    if include_disabled.unwrap_or(false) {
      return Ok(keys);
    }
    filter_enabled_keys(&self.state_store, keys)
  }

  #[napi]
  pub fn set_meme_enabled(&self, key: String, enabled: bool) -> Result<()> {
    let key = sanitize_key(&key)?;
    if get_meme(&key).is_none() {
      return Err(Error::new(
        Status::InvalidArg,
        format!("Unknown meme key: {key}"),
      ));
    }
    self.state_store.set_enabled(&key, enabled)
  }

  #[napi]
  pub fn is_meme_enabled(&self, key: String) -> Result<bool> {
    let key = sanitize_key(&key)?;
    self.state_store.is_enabled(&key)
  }

  #[napi]
  pub fn list_meme_states(&self) -> Result<Vec<MemeState>> {
    self.state_store.list_states()
  }

  #[napi]
  pub fn get_disabled_meme_keys(&self) -> Result<Vec<String>> {
    self.state_store.disabled_keys()
  }

  #[napi]
  pub fn generate_meme(&self, payload: GenerateMemePayload) -> Result<Buffer> {
    let result = self.generate_meme_result_internal(payload)?;
    Ok(result.buffer)
  }

  #[napi]
  pub fn generate_meme_preview(&self, key: String, options: Option<Value>) -> Result<Buffer> {
    let key = sanitize_key(&key)?;
    if !self.state_store.is_enabled(&key)? {
      return Err(Error::new(
        Status::InvalidArg,
        format!("Meme is disabled: {key}"),
      ));
    }
    let meme = get_meme(&key)
      .ok_or_else(|| Error::new(Status::InvalidArg, format!("Unknown meme key: {key}")))?;
    let options = parse_option_map(options.unwrap_or(Value::Object(Default::default())))?;
    let bytes = catch_panic(|| meme.generate_preview(options))
      .and_then(map_meme_err)
      .map_err(bridge_err)?;
    Ok(bytes.into())
  }

  #[napi]
  pub fn generate_meme_detailed(&self, payload: GenerateMemePayload) -> Result<GenerateMemeResult> {
    self.generate_meme_result_internal(payload)
  }

  #[napi]
  pub fn validate_generate_payload(
    &self,
    payload: GenerateMemePayload,
  ) -> Result<GenerateValidationResult> {
    let mut issues = Vec::new();
    let mut required_min_images = 0;
    let mut required_max_images = 0;
    let mut required_min_texts = 0;
    let mut required_max_texts = 0;

    let key = match sanitize_key(&payload.key) {
      Ok(v) => v,
      Err(_) => {
        issues.push(ValidationIssue {
          code: CODE_INVALID_PAYLOAD.to_string(),
          field: "key".to_string(),
          message: "invalid key format, expected [a-zA-Z0-9_-], len 1..128".to_string(),
        });
        return Ok(GenerateValidationResult {
          ok: false,
          issues,
          required_min_images,
          required_max_images,
          required_min_texts,
          required_max_texts,
        });
      }
    };

    if !self.state_store.is_enabled(&key)? {
      issues.push(ValidationIssue {
        code: CODE_MEME_DISABLED.to_string(),
        field: "key".to_string(),
        message: format!("meme is disabled: {key}"),
      });
    }

    let Some(meme) = get_meme(&key) else {
      issues.push(ValidationIssue {
        code: CODE_MEME_NOT_FOUND.to_string(),
        field: "key".to_string(),
        message: format!("unknown meme key: {key}"),
      });
      return Ok(GenerateValidationResult {
        ok: false,
        issues,
        required_min_images,
        required_max_images,
        required_min_texts,
        required_max_texts,
      });
    };

    let info = meme.info();
    required_min_images = info.params.min_images as u32;
    required_max_images = info.params.max_images as u32;
    required_min_texts = info.params.min_texts as u32;
    required_max_texts = info.params.max_texts as u32;

    let image_count = payload.images.as_ref().map_or(0, Vec::len) as u32;
    if image_count < required_min_images || image_count > required_max_images {
      issues.push(ValidationIssue {
        code: CODE_IMAGE_COUNT_MISMATCH.to_string(),
        field: "images".to_string(),
        message: format!(
          "image count mismatch: expected [{required_min_images}, {required_max_images}], got {image_count}"
        ),
      });
    }

    let text_count = payload.texts.as_ref().map_or(0, Vec::len) as u32;
    if text_count < required_min_texts || text_count > required_max_texts {
      issues.push(ValidationIssue {
        code: CODE_TEXT_COUNT_MISMATCH.to_string(),
        field: "texts".to_string(),
        message: format!(
          "text count mismatch: expected [{required_min_texts}, {required_max_texts}], got {text_count}"
        ),
      });
    }

    if let Some(images) = &payload.images {
      if images.len() > MAX_IMAGE_COUNT {
        issues.push(ValidationIssue {
          code: CODE_INVALID_PAYLOAD.to_string(),
          field: "images".to_string(),
          message: format!("too many images, max {MAX_IMAGE_COUNT}"),
        });
      }
      for (idx, image) in images.iter().enumerate() {
        if image.data.is_empty() {
          issues.push(ValidationIssue {
            code: CODE_INVALID_PAYLOAD.to_string(),
            field: format!("images[{idx}].data"),
            message: "image data cannot be empty".to_string(),
          });
        }
      }
    }

    if let Some(texts) = &payload.texts {
      for (idx, text) in texts.iter().enumerate() {
        if let Err(err) = sanitize_plain_text(text, self.max_text_length) {
          issues.push(ValidationIssue {
            code: CODE_INVALID_PAYLOAD.to_string(),
            field: format!("texts[{idx}]"),
            message: err.to_string(),
          });
        }
      }
    }

    validate_options_payload(payload.options.as_ref(), &info, &mut issues);

    let status = self.get_resource_status_for_key(&key)?;
    if !status.available {
      issues.push(ValidationIssue {
        code: status
          .code
          .unwrap_or_else(|| CODE_RESOURCE_MISSING.to_string()),
        field: "resources".to_string(),
        message: status
          .message
          .unwrap_or_else(|| "meme resources are missing".to_string()),
      });
    }

    Ok(GenerateValidationResult {
      ok: issues.is_empty(),
      issues,
      required_min_images,
      required_max_images,
      required_min_texts,
      required_max_texts,
    })
  }

  #[napi]
  pub fn check_resources(&self, base_url: Option<String>) -> Result<()> {
    let base_url = sanitize_optional_url(base_url)?;
    catch_panic(|| {
      resources::check_resources_sync(base_url);
    })
    .map_err(bridge_err)?;
    Ok(())
  }

  #[napi]
  pub fn check_resources_in_background(&self, base_url: Option<String>) -> Result<()> {
    let base_url = sanitize_optional_url(base_url)?;
    catch_panic(|| {
      resources::check_resources_in_background(base_url);
    })
    .map_err(bridge_err)?;
    Ok(())
  }

  #[napi]
  pub fn inspect_image(&self, image: Buffer) -> Result<Value> {
    let image = sanitize_blob(image)?;
    call_image_op_json(|| tools::image_operations::inspect(image))
  }

  #[napi]
  pub fn flip_horizontal(&self, image: Buffer) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::flip_horizontal(image))
  }

  #[napi]
  pub fn flip_vertical(&self, image: Buffer) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::flip_vertical(image))
  }

  #[napi]
  pub fn rotate(&self, image: Buffer, degrees: Option<f64>) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::rotate(image, degrees.map(|v| v as f32)))
  }

  #[napi]
  pub fn resize(&self, image: Buffer, width: Option<i32>, height: Option<i32>) -> Result<Buffer> {
    if width.unwrap_or(1) <= 0 || height.unwrap_or(1) <= 0 {
      return Err(Error::new(
        Status::InvalidArg,
        "width/height must be positive when provided".to_string(),
      ));
    }
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::resize(image, width, height))
  }

  #[napi]
  pub fn crop(
    &self,
    image: Buffer,
    left: Option<i32>,
    top: Option<i32>,
    right: Option<i32>,
    bottom: Option<i32>,
  ) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::crop(image, left, top, right, bottom))
  }

  #[napi]
  pub fn grayscale(&self, image: Buffer) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::grayscale(image))
  }

  #[napi]
  pub fn invert(&self, image: Buffer) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::invert(image))
  }

  #[napi]
  pub fn merge_horizontal(&self, images: Vec<Buffer>) -> Result<Buffer> {
    let images = sanitize_image_vec(images)?;
    call_image_op_bytes(|| tools::image_operations::merge_horizontal(images))
  }

  #[napi]
  pub fn merge_vertical(&self, images: Vec<Buffer>) -> Result<Buffer> {
    let images = sanitize_image_vec(images)?;
    call_image_op_bytes(|| tools::image_operations::merge_vertical(images))
  }

  #[napi]
  pub fn gif_split(&self, image: Buffer) -> Result<Vec<Buffer>> {
    let image = sanitize_blob(image)?;
    let parts = catch_panic(|| tools::image_operations::gif_split(image))
      .and_then(map_meme_err)
      .map_err(bridge_err)?;
    Ok(parts.into_iter().map(Buffer::from).collect())
  }

  #[napi]
  pub fn gif_merge(&self, images: Vec<Buffer>, duration: Option<f64>) -> Result<Buffer> {
    if duration.unwrap_or(0.1) <= 0.0 {
      return Err(Error::new(
        Status::InvalidArg,
        "duration must be > 0".to_string(),
      ));
    }
    let images = sanitize_image_vec(images)?;
    let duration = duration.map(|v| v as f32);
    call_image_op_bytes(|| tools::image_operations::gif_merge(images, duration))
  }

  #[napi]
  pub fn gif_reverse(&self, image: Buffer) -> Result<Buffer> {
    let image = sanitize_blob(image)?;
    call_image_op_bytes(|| tools::image_operations::gif_reverse(image))
  }

  #[napi]
  pub fn gif_change_duration(&self, image: Buffer, duration: f64) -> Result<Buffer> {
    if duration <= 0.0 {
      return Err(Error::new(
        Status::InvalidArg,
        "duration must be > 0".to_string(),
      ));
    }
    let image = sanitize_blob(image)?;
    let duration = duration as f32;
    call_image_op_bytes(|| tools::image_operations::gif_change_duration(image, duration))
  }

  #[napi]
  pub fn render_meme_list(&self, params: Option<Value>) -> Result<Buffer> {
    let params = if let Some(params) = params {
      serde_json::from_value::<tools::RenderMemeListParams>(params).map_err(json_err)?
    } else {
      tools::RenderMemeListParams::default()
    };
    let bytes = catch_panic(|| tools::render_meme_list(params))
      .and_then(map_meme_err)
      .map_err(bridge_err)?;
    Ok(bytes.into())
  }

  #[napi]
  pub fn render_meme_statistics(&self, params: Value) -> Result<Buffer> {
    let params =
      serde_json::from_value::<tools::RenderMemeStatisticsParams>(params).map_err(json_err)?;
    if params.data.is_empty() {
      return Err(Error::new(
        Status::InvalidArg,
        "statistics data cannot be empty".to_string(),
      ));
    }
    if params
      .data
      .iter()
      .any(|(label, count)| sanitize_plain_text(label, 128).is_err() || *count < 0)
    {
      return Err(Error::new(
        Status::InvalidArg,
        "statistics labels must be valid text and count must be >= 0".to_string(),
      ));
    }
    let bytes = catch_panic(|| tools::render_meme_statistics(params))
      .and_then(map_meme_err)
      .map_err(bridge_err)?;
    Ok(bytes.into())
  }

  #[napi]
  pub fn get_resource_status(&self, key: Option<String>) -> Result<Vec<ResourceStatus>> {
    if let Some(key) = key {
      let key = sanitize_key(&key)?;
      return Ok(vec![self.get_resource_status_for_key(&key)?]);
    }

    let mut out = Vec::new();
    for meme in get_memes() {
      out.push(self.get_resource_status_for_key(&meme.key())?);
    }
    Ok(out)
  }

  #[napi]
  pub fn generate_random(
    &self,
    payload: Option<GenerateRandomPayload>,
  ) -> Result<GenerateMemeResult> {
    let payload = payload.unwrap_or(GenerateRandomPayload {
      filters: None,
      images: None,
      texts: None,
      options: None,
    });

    let filters = payload.filters.unwrap_or(RandomGenerateFilter {
      require_images: None,
      min_texts: None,
      max_texts: None,
      exclude_keys: None,
      include_disabled: None,
    });

    let exclude_keys = filters
      .exclude_keys
      .unwrap_or_default()
      .into_iter()
      .filter_map(|k| sanitize_key(&k).ok())
      .collect::<Vec<_>>();

    let base_images = sanitize_images(payload.images.unwrap_or_default())?;
    let base_texts = sanitize_texts(payload.texts.unwrap_or_default(), self.max_text_length)?;
    let base_options =
      parse_option_map(payload.options.unwrap_or(Value::Object(Default::default())))?;

    let input_images = base_images.len() as u32;
    let input_texts = base_texts.len() as u32;
    let require_images = filters.require_images.unwrap_or(input_images > 0);
    let min_texts = filters.min_texts.unwrap_or(0);
    let max_texts = filters.max_texts.unwrap_or(u32::MAX);
    let include_disabled = filters.include_disabled.unwrap_or(false);

    let mut candidates = Vec::new();
    for meme in get_memes() {
      let key = meme.key();
      if exclude_keys.iter().any(|k| k == &key) {
        continue;
      }
      let enabled = self.state_store.is_enabled(&key)?;
      if !include_disabled && !enabled {
        continue;
      }
      let info = meme.info();
      let p = info.params;
      if require_images && p.max_images == 0 {
        continue;
      }
      if (p.max_texts as u32) < min_texts || (p.min_texts as u32) > max_texts {
        continue;
      }
      if input_images < p.min_images as u32 || input_images > p.max_images as u32 {
        continue;
      }
      if input_texts < p.min_texts as u32 || input_texts > p.max_texts as u32 {
        continue;
      }
      candidates.push(key);
    }

    if candidates.is_empty() {
      return Err(new_coded_error(
        CODE_RANDOM_GENERATION_FAILED,
        "no meme candidates matched the random filters".to_string(),
        Status::InvalidArg,
      ));
    }

    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as usize)
      .unwrap_or(0);
    let start = seed % candidates.len();

    let mut last_error = None;
    for i in 0..candidates.len() {
      let key = candidates[(start + i) % candidates.len()].clone();
      match generate_meme_by_parts(
        &self.state_store,
        &key,
        &base_images,
        &base_texts,
        &base_options,
      ) {
        Ok(result) => return Ok(result),
        Err(err) => {
          last_error = Some(err.to_string());
        }
      }
    }

    Err(new_coded_error(
      CODE_RANDOM_GENERATION_FAILED,
      format!(
        "all {} candidates failed, last error: {}",
        candidates.len(),
        last_error.unwrap_or_else(|| "unknown".to_string())
      ),
      Status::GenericFailure,
    ))
  }
}

impl MemeGenerator {
  fn generate_meme_result_internal(
    &self,
    payload: GenerateMemePayload,
  ) -> Result<GenerateMemeResult> {
    let key = sanitize_key(&payload.key)?;
    let images = sanitize_images(payload.images.unwrap_or_default())?;
    let texts = sanitize_texts(payload.texts.unwrap_or_default(), self.max_text_length)?;
    let options = parse_option_map(payload.options.unwrap_or(Value::Object(Default::default())))?;
    generate_meme_by_parts(&self.state_store, &key, &images, &texts, &options)
  }

  fn get_resource_status_for_key(&self, key: &str) -> Result<ResourceStatus> {
    let enabled = self.state_store.is_enabled(key)?;
    let Some(meme) = get_meme(key) else {
      return Ok(ResourceStatus {
        key: key.to_string(),
        enabled,
        available: false,
        code: Some(CODE_MEME_NOT_FOUND.to_string()),
        message: Some(format!("unknown meme key: {key}")),
      });
    };

    let default_options = default_options_from_info(&meme.info());
    let result = catch_panic(|| meme.generate_preview(default_options));
    match result.and_then(map_meme_err) {
      Ok(_) => Ok(ResourceStatus {
        key: key.to_string(),
        enabled,
        available: true,
        code: None,
        message: None,
      }),
      Err(err) => {
        if err.code == CODE_ASSET_MISSING {
          Ok(ResourceStatus {
            key: key.to_string(),
            enabled,
            available: false,
            code: Some(CODE_RESOURCE_MISSING.to_string()),
            message: Some(err.message),
          })
        } else {
          Ok(ResourceStatus {
            key: key.to_string(),
            enabled,
            available: true,
            code: None,
            message: None,
          })
        }
      }
    }
  }
}

fn filter_enabled_keys(state_store: &StateStore, keys: Vec<String>) -> Result<Vec<String>> {
  let mut out = Vec::with_capacity(keys.len());
  for key in keys {
    if state_store.is_enabled(&key)? {
      out.push(key);
    }
  }
  Ok(out)
}

fn generate_meme_by_parts(
  state_store: &StateStore,
  key: &str,
  images: &[Image],
  texts: &[String],
  options: &HashMap<String, OptionValue>,
) -> Result<GenerateMemeResult> {
  if !state_store.is_enabled(key)? {
    return Err(new_coded_error(
      CODE_MEME_DISABLED,
      format!("meme is disabled: {key}"),
      Status::InvalidArg,
    ));
  }
  let meme = get_meme(key).ok_or_else(|| {
    new_coded_error(
      CODE_MEME_NOT_FOUND,
      format!("unknown meme key: {key}"),
      Status::InvalidArg,
    )
  })?;

  let images_for_gen = images
    .iter()
    .map(|i| Image {
      name: i.name.clone(),
      data: i.data.clone(),
    })
    .collect::<Vec<_>>();
  let texts_for_gen = texts.to_vec();
  let options_for_gen = options.clone();

  let buffer = catch_panic(|| meme.generate(images_for_gen, texts_for_gen, options_for_gen))
    .and_then(map_meme_err)
    .map_err(bridge_err)?;

  Ok(GenerateMemeResult {
    key: key.to_string(),
    mime: detect_mime(&buffer).to_string(),
    used_images: images.len() as u32,
    used_texts: texts.len() as u32,
    buffer: buffer.into(),
  })
}

fn detect_mime(buffer: &[u8]) -> &'static str {
  if buffer.len() >= 6 && (&buffer[0..6] == b"GIF87a" || &buffer[0..6] == b"GIF89a") {
    "image/gif"
  } else {
    "image/png"
  }
}

fn parser_flags_to_dto(flags: ParserFlags) -> ParserFlagsDto {
  ParserFlagsDto {
    short: flags.short,
    long: flags.long,
    short_aliases: flags
      .short_aliases
      .into_iter()
      .map(|c| c.to_string())
      .collect(),
    long_aliases: flags.long_aliases,
  }
}

fn meme_option_to_dto(option: MemeOption) -> MemeOptionDto {
  match option {
    MemeOption::Boolean {
      name,
      default,
      description,
      parser_flags,
    } => MemeOptionDto {
      option_type: "boolean".to_string(),
      name,
      description,
      parser_flags: parser_flags_to_dto(parser_flags),
      default_value: default.map(Value::Bool),
      choices: None,
      minimum: None,
      maximum: None,
    },
    MemeOption::String {
      name,
      default,
      choices,
      description,
      parser_flags,
    } => MemeOptionDto {
      option_type: "string".to_string(),
      name,
      description,
      parser_flags: parser_flags_to_dto(parser_flags),
      default_value: default.map(Value::String),
      choices,
      minimum: None,
      maximum: None,
    },
    MemeOption::Integer {
      name,
      default,
      minimum,
      maximum,
      description,
      parser_flags,
    } => MemeOptionDto {
      option_type: "integer".to_string(),
      name,
      description,
      parser_flags: parser_flags_to_dto(parser_flags),
      default_value: default.map(|v| Value::Number(v.into())),
      choices: None,
      minimum: minimum.map(|v| v as f64),
      maximum: maximum.map(|v| v as f64),
    },
    MemeOption::Float {
      name,
      default,
      minimum,
      maximum,
      description,
      parser_flags,
    } => MemeOptionDto {
      option_type: "float".to_string(),
      name,
      description,
      parser_flags: parser_flags_to_dto(parser_flags),
      default_value: default
        .and_then(|v| serde_json::Number::from_f64(v as f64))
        .map(Value::Number),
      choices: None,
      minimum: minimum.map(|v| v as f64),
      maximum: maximum.map(|v| v as f64),
    },
  }
}

fn meme_info_to_dto(info: &MemeInfo, enabled: bool) -> MemeInfoDto {
  MemeInfoDto {
    key: info.key.clone(),
    params: MemeParamsDto {
      min_images: info.params.min_images as u32,
      max_images: info.params.max_images as u32,
      min_texts: info.params.min_texts as u32,
      max_texts: info.params.max_texts as u32,
      default_texts: info.params.default_texts.clone(),
      options: info
        .params
        .options
        .clone()
        .into_iter()
        .map(meme_option_to_dto)
        .collect(),
    },
    keywords: info.keywords.clone(),
    shortcuts: info
      .shortcuts
      .iter()
      .map(|s| MemeShortcutDto {
        pattern: s.pattern.clone(),
        humanized: s.humanized.clone(),
        names: s.names.clone(),
        texts: s.texts.clone(),
        options: serde_json::to_value(&s.options).unwrap_or(Value::Null),
      })
      .collect(),
    tags: {
      let mut tags = info.tags.iter().cloned().collect::<Vec<_>>();
      tags.sort();
      tags
    },
    date_created: info.date_created.to_rfc3339(),
    date_modified: info.date_modified.to_rfc3339(),
    enabled,
  }
}

fn default_options_from_info(info: &MemeInfo) -> HashMap<String, OptionValue> {
  let mut options = HashMap::new();
  for option in &info.params.options {
    match option {
      MemeOption::Boolean { name, default, .. } => {
        if let Some(v) = default {
          options.insert(name.clone(), OptionValue::Boolean(*v));
        }
      }
      MemeOption::String { name, default, .. } => {
        if let Some(v) = default {
          options.insert(name.clone(), OptionValue::String(v.clone()));
        }
      }
      MemeOption::Integer { name, default, .. } => {
        if let Some(v) = default {
          options.insert(name.clone(), OptionValue::Integer(*v));
        }
      }
      MemeOption::Float { name, default, .. } => {
        if let Some(v) = default {
          options.insert(name.clone(), OptionValue::Float(*v));
        }
      }
    }
  }
  options
}

fn validate_options_payload(
  options: Option<&Value>,
  info: &MemeInfo,
  issues: &mut Vec<ValidationIssue>,
) {
  let schema = info
    .params
    .options
    .iter()
    .map(|opt| match opt {
      MemeOption::Boolean { name, default, .. } => (name.clone(), "boolean", default.is_none()),
      MemeOption::String { name, default, .. } => (name.clone(), "string", default.is_none()),
      MemeOption::Integer { name, default, .. } => (name.clone(), "integer", default.is_none()),
      MemeOption::Float { name, default, .. } => (name.clone(), "float", default.is_none()),
    })
    .collect::<Vec<_>>();

  let empty = Value::Object(Default::default());
  let input = options.unwrap_or(&empty);
  let Some(input_obj) = input.as_object() else {
    issues.push(ValidationIssue {
      code: CODE_INVALID_OPTION.to_string(),
      field: "options".to_string(),
      message: "options must be a plain object".to_string(),
    });
    return;
  };

  for (name, _, required) in &schema {
    if *required && !input_obj.contains_key(name) {
      issues.push(ValidationIssue {
        code: CODE_INVALID_OPTION.to_string(),
        field: format!("options.{name}"),
        message: "missing required option".to_string(),
      });
    }
  }

  for (name, value) in input_obj {
    let Some((_, typ, _)) = schema.iter().find(|(n, _, _)| n == name) else {
      issues.push(ValidationIssue {
        code: CODE_INVALID_OPTION.to_string(),
        field: format!("options.{name}"),
        message: "unknown option key".to_string(),
      });
      continue;
    };

    let type_ok = match *typ {
      "boolean" => value.is_boolean(),
      "string" => value.is_string(),
      "integer" => value.as_i64().is_some(),
      "float" => value.as_f64().is_some(),
      _ => false,
    };
    if !type_ok {
      issues.push(ValidationIssue {
        code: CODE_INVALID_OPTION.to_string(),
        field: format!("options.{name}"),
        message: format!("invalid option type, expected {typ}"),
      });
    }
  }
}

fn call_image_op_bytes<F>(f: F) -> Result<Buffer>
where
  F: FnOnce() -> std::result::Result<Vec<u8>, meme_generator::error::Error>,
{
  let bytes = catch_panic(f).and_then(map_meme_err).map_err(bridge_err)?;
  Ok(bytes.into())
}

fn call_image_op_json<T, F>(f: F) -> Result<Value>
where
  T: Serialize,
  F: FnOnce() -> std::result::Result<T, meme_generator::error::Error>,
{
  let value = catch_panic(f).and_then(map_meme_err).map_err(bridge_err)?;
  serde_json::to_value(value).map_err(json_err)
}

fn catch_panic<T, F>(f: F) -> std::result::Result<T, BridgeFailure>
where
  F: FnOnce() -> T,
{
  panic::catch_unwind(AssertUnwindSafe(f)).map_err(|_| BridgeFailure {
    code: CODE_INTERNAL_PANIC.to_string(),
    message: "panic captured in rust addon call, request aborted for process safety".to_string(),
  })
}

fn map_meme_err<T>(
  value: std::result::Result<T, meme_generator::error::Error>,
) -> std::result::Result<T, BridgeFailure> {
  value.map_err(map_meme_error)
}

fn bridge_err(err: BridgeFailure) -> Error {
  new_coded_error(&err.code, err.message, Status::GenericFailure)
}

fn sql_err(err: rusqlite::Error) -> Error {
  Error::new(Status::GenericFailure, format!("sqlite error: {err}"))
}

fn io_err(err: std::io::Error) -> Error {
  Error::new(Status::GenericFailure, format!("io error: {err}"))
}

fn json_err(err: serde_json::Error) -> Error {
  Error::new(Status::InvalidArg, format!("invalid json payload: {err}"))
}

#[derive(Debug, Clone)]
struct BridgeFailure {
  code: String,
  message: String,
}

fn map_meme_error(err: meme_generator::error::Error) -> BridgeFailure {
  use meme_generator::error::Error as MemeErr;
  let code = match &err {
    MemeErr::ImageNumberMismatch(_, _, _) => CODE_IMAGE_COUNT_MISMATCH,
    MemeErr::TextNumberMismatch(_, _, _) => CODE_TEXT_COUNT_MISMATCH,
    MemeErr::ImageAssetMissing(_) => CODE_ASSET_MISSING,
    MemeErr::DeserializeError(_) => CODE_INVALID_OPTION,
    _ => "GENERATION_FAILED",
  };
  BridgeFailure {
    code: code.to_string(),
    message: err.to_string(),
  }
}

fn new_coded_error(code: &str, message: String, status: Status) -> Error {
  Error::new(status, format!("[{code}] {message}"))
}

fn sanitize_key(input: &str) -> Result<String> {
  let value = input.trim();
  if value.is_empty() || value.len() > 128 {
    return Err(Error::new(
      Status::InvalidArg,
      "meme key length must be in [1, 128]".to_string(),
    ));
  }
  if !value
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
  {
    return Err(Error::new(
      Status::InvalidArg,
      "meme key only supports [a-zA-Z0-9_-]".to_string(),
    ));
  }
  Ok(value.to_string())
}

fn sanitize_query(input: &str) -> Result<String> {
  sanitize_plain_text(input, 128)
}

fn sanitize_plain_text(input: &str, max_len: usize) -> Result<String> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Err(Error::new(
      Status::InvalidArg,
      "text cannot be empty".to_string(),
    ));
  }
  if trimmed.chars().count() > max_len {
    return Err(Error::new(
      Status::InvalidArg,
      format!("text is too long, max {max_len} chars"),
    ));
  }
  if trimmed
    .chars()
    .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t')))
  {
    return Err(Error::new(
      Status::InvalidArg,
      "text contains illegal control characters".to_string(),
    ));
  }
  Ok(trimmed.to_string())
}

fn sanitize_optional_url(input: Option<String>) -> Result<Option<String>> {
  if let Some(url) = input {
    let url = sanitize_plain_text(&url, 1024)?;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
      return Err(Error::new(
        Status::InvalidArg,
        "baseUrl must start with http:// or https://".to_string(),
      ));
    }
    return Ok(Some(url));
  }
  Ok(None)
}

fn sanitize_blob(data: Buffer) -> Result<Vec<u8>> {
  if data.is_empty() {
    return Err(Error::new(
      Status::InvalidArg,
      "binary payload cannot be empty".to_string(),
    ));
  }
  if data.len() > MAX_BLOB_SIZE {
    return Err(Error::new(
      Status::InvalidArg,
      format!("binary payload too large, max {} bytes", MAX_BLOB_SIZE),
    ));
  }
  Ok(data.to_vec())
}

fn sanitize_image_vec(images: Vec<Buffer>) -> Result<Vec<Vec<u8>>> {
  if images.is_empty() {
    return Err(Error::new(
      Status::InvalidArg,
      "images cannot be empty".to_string(),
    ));
  }
  if images.len() > MAX_IMAGE_COUNT {
    return Err(Error::new(
      Status::InvalidArg,
      format!("too many images, max {MAX_IMAGE_COUNT}"),
    ));
  }
  images.into_iter().map(sanitize_blob).collect()
}

fn sanitize_images(images: Vec<InputImagePayload>) -> Result<Vec<Image>> {
  if images.len() > MAX_IMAGE_COUNT {
    return Err(Error::new(
      Status::InvalidArg,
      format!("too many images, max {MAX_IMAGE_COUNT}"),
    ));
  }
  let mut out = Vec::with_capacity(images.len());
  for image in images {
    let name = image.name.unwrap_or_default();
    if !name.is_empty() {
      sanitize_plain_text(&name, 64)?;
    }
    let data = sanitize_blob(image.data)?;
    out.push(Image { name, data });
  }
  Ok(out)
}

fn sanitize_texts(texts: Vec<String>, max_len: usize) -> Result<Vec<String>> {
  if texts.len() > 64 {
    return Err(Error::new(
      Status::InvalidArg,
      "too many text entries, max 64".to_string(),
    ));
  }
  texts
    .into_iter()
    .map(|t| sanitize_plain_text(&t, max_len))
    .collect()
}

fn parse_option_map(value: Value) -> Result<HashMap<String, OptionValue>> {
  let mut map = HashMap::new();
  let obj = value.as_object().ok_or_else(|| {
    new_coded_error(
      CODE_INVALID_OPTION,
      "options must be a plain object".to_string(),
      Status::InvalidArg,
    )
  })?;
  for (key, value) in obj {
    let key = sanitize_key(key)?;
    let option_value = match value {
      Value::Bool(v) => OptionValue::Boolean(*v),
      Value::String(v) => OptionValue::String(sanitize_plain_text(v, 2048)?),
      Value::Number(v) => {
        if let Some(i) = v.as_i64() {
          if !(i32::MIN as i64..=i32::MAX as i64).contains(&i) {
            return Err(new_coded_error(
              CODE_INVALID_OPTION,
              format!("option {key} integer is out of i32 range"),
              Status::InvalidArg,
            ));
          }
          OptionValue::Integer(i as i32)
        } else if let Some(f) = v.as_f64() {
          OptionValue::Float(f as f32)
        } else {
          return Err(new_coded_error(
            CODE_INVALID_OPTION,
            format!("option {key} has unsupported numeric value"),
            Status::InvalidArg,
          ));
        }
      }
      _ => {
        return Err(new_coded_error(
          CODE_INVALID_OPTION,
          format!("option {key} only supports boolean/string/number"),
          Status::InvalidArg,
        ));
      }
    };
    map.insert(key, option_value);
  }
  Ok(map)
}
