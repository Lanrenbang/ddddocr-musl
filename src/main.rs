use clap::Parser;
use ddddocr_musl::*;
use salvo::prelude::*;
use salvo::oapi::extract::JsonBody;
use base64::prelude::*;
use std::path::PathBuf;
use std::sync::OnceLock;
use anyhow::Context;
use tokio::task::spawn_blocking;
use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use std::sync::LazyLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use salvo::catcher::Catcher;

// Global singletons
static ARGS: OnceLock<Args> = OnceLock::new();
static OCR: LazyLock<RwLock<Option<Arc<Ddddocr<'static>>>>> = LazyLock::new(|| RwLock::new(None));
static DET: LazyLock<RwLock<Option<Arc<Ddddocr<'static>>>>> = LazyLock::new(|| RwLock::new(None));
static SLIDE_ENABLED: AtomicBool = AtomicBool::new(true);
static CACHE: LazyLock<Mutex<LruCache<String, Vec<String>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(NonZeroUsize::new(10).unwrap())));

#[derive(Parser, Debug, Clone)]
struct Args {
    /// Listen address. Supported formats:
    /// 1. Unix Socket: Starts with "/" (e.g., "/tmp/ddddocr.sock")
    /// 2. Port Number: Pure number (e.g., "8080" -> "0.0.0.0:8080")
    /// 3. TCP Address: IP:PORT (e.g., "127.0.0.1:8000")
    #[arg(long, default_value = "0.0.0.0:8000")]
    address: String,

    #[arg(long, default_value = "model/common.onnx")]
    ocr_path: PathBuf,

    #[arg(long, default_value = "model/common_det.onnx")]
    det_path: PathBuf,

    #[arg(long)]
    disable_ocr: bool,
    
    #[arg(long)]
    disable_det: bool,

    #[arg(long)]
    disable_slide: bool,

    #[arg(long)]
    ocr_charset_range: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct OCRRequest {
    image: String,
    png_fix: Option<bool>,
    probability: Option<bool>,
    charset_range: Option<String>,
    color_filter: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct OCRResponse {
    text: String,
    probability: Option<Vec<Vec<f32>>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct DETRequest { image: String }

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct DETResponse { bboxes: Vec<Vec<u32>> }

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct SlideRequest { target_image: String, background_image: String, simple_target: Option<bool> }

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct SlideResponse { 
    target: Vec<u32>, 
    #[serde(rename = "target_x")]
    target_x: u32, 
    #[serde(rename = "target_y")]
    target_y: u32 
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct CompareRequest { target_image: String, background_image: String }

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct CompareResponse { x: u32, y: u32 }

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct ToggleRequest {
    ocr: Option<bool>,
    det: Option<bool>,
    slide: Option<bool>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct StatusResponse {
    service_status: String,
    enabled_features: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct APIResponse<T> {
    code: u16,
    msg: String,
    data: Option<T>,
}

fn load_ocr_internal() -> anyhow::Result<Ddddocr<'static>> {
    let args = ARGS.get().context("Args not initialized")?;
    if !args.ocr_path.exists() {
        anyhow::bail!("OCR model not found at {:?}", args.ocr_path);
    }
    println!("Loading OCR model from {:?}", args.ocr_path);
    let model = std::fs::read(&args.ocr_path).context("Reading OCR model")?;
    let mut json_path = args.ocr_path.clone();
    json_path.set_extension("json");
    let json_str = std::fs::read_to_string(&json_path).context("Reading OCR charset")?;
    let charset: Charset = serde_json::from_str(&json_str)?;
    
    let mut instance = Ddddocr::new(model, charset)?;
    if let Some(range) = &args.ocr_charset_range {
        instance.set_ranges(range.as_str());
    }
    println!("OCR loaded.");
    Ok(instance)
}

fn load_det_internal() -> anyhow::Result<Ddddocr<'static>> {
    let args = ARGS.get().context("Args not initialized")?;
    if !args.det_path.exists() {
        anyhow::bail!("DET model not found at {:?}", args.det_path);
    }
    println!("Loading DET model from {:?}", args.det_path);
    let model = std::fs::read(&args.det_path).context("Reading DET model")?;
    let instance = Ddddocr::new_det(model)?;
    println!("DET loaded.");
    Ok(instance)
}

#[endpoint]
async fn ocr(req: JsonBody<OCRRequest>) -> anyhow::Result<Json<APIResponse<OCRResponse>>> {
    let ocr_lock = OCR.read().await;
    let ocr_instance = ocr_lock.as_ref().context("OCR not enabled")?;
    
    let bytes = BASE64_STANDARD.decode(&req.image).context("Base64 decode failed")?;
    
    let filter = if let Some(v) = req.color_filter.clone() {
        Some(serde_json::from_value::<ColorFilter>(v).context("Invalid color_filter format")?)
    } else {
        None
    };

    let charset_range = if let Some(ref v) = req.charset_range {
        let ocr_charset_range = match v.as_str() {
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" => {
                CharsetRange::from(v.parse::<i32>().unwrap())
            }
            v => CharsetRange::from(v),
        };

        // Use cache for calculated ranges
        let mut cache = CACHE.lock().await;
        let calculated = if let Some(cached) = cache.get(v) {
            cached.clone()
        } else {
            let calculated = ocr_instance.calc_ranges(ocr_charset_range);
            cache.put(v.clone(), calculated.clone());
            calculated
        };
        Some(CharsetRange::Charset(calculated))
    } else {
        None
    };
    
    let png_fix = req.png_fix.unwrap_or(false);
    let need_prob = req.probability.unwrap_or(false);

    // We cannot pass ocr_instance (reference) to spawn_blocking because it's not 'static.
    // We have to run it here or clone what is needed.
    // Ddddocr is Send+Sync. But we hold a ReadGuard.
    // We cannot easily move the ReadGuard into spawn_blocking.
    // However, Ddddocr is designed to be shared. 
    // Since we can't move the reference across thread boundary if it borrows from stack (guard),
    // we might need to change Ddddocr to be Arc-wrapped internally or wrap the whole thing in Arc.
    // Wait, OCR is RwLock<Option<Ddddocr>>.
    // If we want to use it in spawn_blocking, we usually need an Arc.
    // BUT, Ddddocr methods take &self.
    // If we run strictly cpu bound work, we can use block_in_place or just run it?
    // No, running heavy compute in async fn blocks the executor.
    // Ideally Ddddocr should be wrapped in Arc so we can clone the Arc and pass it to spawn_blocking.
    // Let's do that: Change globals to Arc<Ddddocr>.
    
    // Actually, let's stick to the previous implementation structure where we didn't use Arc but spawn_blocking?
    // In previous code: `let ocr_instance = OCR.get()...` returns `&Ddddocr`.
    // `spawn_blocking(move || ...)` worked because `&Ddddocr` was from `OnceLock` which is static ref.
    // Now we have `RwLockReadGuard`. We can't pass reference to guard to another thread easily.
    // Best approach: Wrap Ddddocr in Arc.
    // `static OCR: LazyLock<RwLock<Option<Arc<Ddddocr<'static>>>>>`
    
    // BUT, I can't change `src/lib.rs` easily right now (I can, but prefer not to unless needed).
    // Let's verify: `Ddddocr` holds `Mutex<Session>`.
    // If I wrap it in Arc, `Arc<Ddddocr>` is cheap to clone.
    // So `RwLock<Option<Arc<Ddddocr>>>` is the way.
    
    // Refactoring to use Arc.
    let ocr_instance = ocr_instance.clone(); // Clone the Arc
    drop(ocr_lock); // Release lock early

    let (text, probability) = spawn_blocking(move || {
        let prob = ocr_instance.classification_probability_with_options(&bytes, png_fix, filter, charset_range)?;
        Ok::<(String, Option<Vec<Vec<f32>>>), anyhow::Error>((
            prob.clone().get_text().to_string(),
            need_prob.then_some(prob.probability),
        ))
    }).await.context("Task join failed")??;

    Ok(Json(APIResponse {
        code: 200, msg: "success".into(),
        data: Some(OCRResponse {
            text,
            probability,
        })
    }))
}

#[endpoint]
async fn det(req: JsonBody<DETRequest>) -> anyhow::Result<Json<APIResponse<DETResponse>>> {
    let det_lock = DET.read().await;
    let det_instance = det_lock.as_ref().context("DET not enabled")?.clone();
    drop(det_lock);

    let bytes = BASE64_STANDARD.decode(&req.image).context("Base64 decode failed")?;
    let boxes = spawn_blocking(move || det_instance.detection(&bytes)).await.context("Task join failed")??;
    
    Ok(Json(APIResponse {
        code: 200, msg: "success".into(),
        data: Some(DETResponse {
            bboxes: boxes.into_iter().map(|b| vec![b.x1, b.y1, b.x2, b.y2]).collect(),
        })
    }))
}

#[endpoint]
async fn slide(req: JsonBody<SlideRequest>) -> anyhow::Result<Json<APIResponse<SlideResponse>>> {
    if !SLIDE_ENABLED.load(Ordering::Relaxed) {
        return Err(anyhow::anyhow!("Slide feature is disabled").into());
    }
    let target = BASE64_STANDARD.decode(&req.target_image).context("Base64 decode failed")?;
    let bg = BASE64_STANDARD.decode(&req.background_image).context("Base64 decode failed")?;
    let simple = req.simple_target.unwrap_or(false);
    
    let res = spawn_blocking(move || {
        if simple {
            simple_slide_match(&target, &bg)
        } else {
            slide_match(&target, &bg)
        }
    }).await.context("Task join failed")??;
    
    Ok(Json(APIResponse {
        code: 200, msg: "success".into(),
        data: Some(SlideResponse {
            target: vec![res.x1, res.y1, res.x2, res.y2],
            target_x: res.target_x, 
            target_y: res.target_y
        })
    }))
}

#[endpoint]
async fn compare(req: JsonBody<CompareRequest>) -> anyhow::Result<Json<APIResponse<CompareResponse>>> {
    if !SLIDE_ENABLED.load(Ordering::Relaxed) {
        return Err(anyhow::anyhow!("Slide feature is disabled").into());
    }
    let target = BASE64_STANDARD.decode(&req.target_image).context("Base64 decode failed")?;
    let bg = BASE64_STANDARD.decode(&req.background_image).context("Base64 decode failed")?;
    
    let (x, y) = spawn_blocking(move || slide_comparison(&target, &bg)).await.context("Task join failed")??;
    
    Ok(Json(APIResponse {
        code: 200, msg: "success".into(),
        data: Some(CompareResponse { x, y })
    }))
}

#[endpoint]
async fn toggle_feature(req: JsonBody<ToggleRequest>) -> Json<APIResponse<()>> {
    if let Some(val) = req.slide {
        SLIDE_ENABLED.store(val, Ordering::Relaxed);
    }

    if let Some(val) = req.ocr {
        if val {
            let needs_load = OCR.read().await.is_none();
            if needs_load {
                // Load outside lock
                match spawn_blocking(load_ocr_internal).await {
                    Ok(Ok(instance)) => {
                         *OCR.write().await = Some(std::sync::Arc::new(instance));
                    },
                    Ok(Err(e)) => println!("Failed to enable OCR: {:?}", e),
                    Err(e) => println!("Join error: {:?}", e),
                }
            }
        } else {
            *OCR.write().await = None;
        }
    }

    if let Some(val) = req.det {
        if val {
            let needs_load = DET.read().await.is_none();
            if needs_load {
                 match spawn_blocking(load_det_internal).await {
                    Ok(Ok(instance)) => {
                         *DET.write().await = Some(std::sync::Arc::new(instance));
                    },
                    Ok(Err(e)) => println!("Failed to enable DET: {:?}", e),
                    Err(e) => println!("Join error: {:?}", e),
                }
            }
        } else {
            *DET.write().await = None;
        }
    }

    Json(APIResponse {
        code: 200,
        msg: "success".to_string(),
        data: None,
    })
}

#[endpoint]
async fn status() -> Json<APIResponse<StatusResponse>> {
    let mut enabled = Vec::new();
    if OCR.read().await.is_some() { enabled.push("ocr".to_string()); }
    if DET.read().await.is_some() { enabled.push("det".to_string()); }
    if SLIDE_ENABLED.load(Ordering::Relaxed) { enabled.push("slide".to_string()); }

    Json(APIResponse {
        code: 200,
        msg: "success".to_string(),
        data: Some(StatusResponse {
            service_status: "running".to_string(),
            enabled_features: enabled,
        })
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
struct ErrorResponse {
    code: u16,
    msg: String,
    err_text: Option<String>,
}

#[handler]
async fn custom_catcher(res: &mut Response, ctrl: &mut FlowCtrl) {
    if let Some(s_code) = res.status_code {
        let code = s_code.as_u16();
        let msg = s_code.canonical_reason().unwrap_or("Unknown Error").to_string();
        let err_text = if code == 500 {
            // Try to get detailed error if available? 
            // Salvo might not expose the inner error easily in catcher, 
            // but we can check if body is already set?
            // For now, just generic 500.
            Some("Internal Server Error".to_string())
        } else {
            None
        };
        
        let error_response = ErrorResponse {
            code,
            msg,
            err_text,
        };
        res.render(Json(error_response));
        ctrl.skip_rest();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    salvo::http::request::set_global_secure_max_size(50 * 1024 * 1024);
    #[cfg(feature = "tract")]
    {
        ort::set_api(ort_tract::api());
    }
    tracing_subscriber::fmt().init();
    let args = Args::parse();
    ARGS.set(args.clone()).expect("Failed to set args");

    SLIDE_ENABLED.store(!args.disable_slide, Ordering::Relaxed);

    if !args.disable_ocr {
        match load_ocr_internal() {
            Ok(inst) => { *OCR.write().await = Some(std::sync::Arc::new(inst)); },
            Err(e) => println!("Init OCR warning: {:?}", e),
        }
    }

    if !args.disable_det {
        match load_det_internal() {
            Ok(inst) => { *DET.write().await = Some(std::sync::Arc::new(inst)); },
            Err(e) => println!("Init DET warning: {:?}", e),
        }
    }

    // Register all routes; feature flags handled inside
    let router = Router::new()
        .push(Router::with_path("ocr").post(ocr))
        .push(Router::with_path("det").post(det))
        .push(Router::with_path("slide-match").post(slide))
        .push(Router::with_path("slide-comparison").post(compare))
        .push(Router::with_path("toggle-feature").post(toggle_feature))
        .push(Router::with_path("status").get(status));
        
    let doc = OpenApi::new("ddddocr-musl", "0.1.0").merge_router(&router);
    let router = router
        .unshift(doc.into_router("/api-doc/openapi.json"))
        .unshift(SwaggerUi::new("/api-doc/openapi.json").into_router("/docs"));

    let service = Service::new(router).catcher(Catcher::default().hoop(custom_catcher));

    if args.address.starts_with("/") {
        let path = PathBuf::from(&args.address);
        println!("Listening on unix socket: {:?}", path);
        if path.exists() { std::fs::remove_file(&path)?; }
        let acceptor = salvo::conn::UnixListener::new(path).bind().await;
        let server = Server::new(acceptor);
        
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = server.serve(service) => {},
            _ = tokio::signal::ctrl_c() => {
                println!("Shutting down (SIGINT)...");
            },
            _ = terminate.recv() => {
                println!("Shutting down (SIGTERM)...");
            }
        }
    } else {
        let addr_str = if args.address.parse::<u16>().is_ok() {
            format!("0.0.0.0:{}", args.address)
        } else {
            args.address.clone()
        };
        println!("Listening on tcp: {}", addr_str);
        let acceptor = TcpListener::new(addr_str).bind().await;
        let server = Server::new(acceptor);
        
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = server.serve(service) => {},
            _ = tokio::signal::ctrl_c() => {
                 println!("Shutting down (SIGINT)...");
            },
            _ = terminate.recv() => {
                 println!("Shutting down (SIGTERM)...");
            }
        }
    }

    Ok(())
}

