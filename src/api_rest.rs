use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::app_state::AppState;
use crate::app::AppMessage;

// --- Data Models ---

#[derive(Serialize, Clone, ToSchema)] 
pub struct StopStatusResponse {
    /// The internal index of the stop
    index: usize,
    /// The name of the stop (e.g., "Principal 8'")
    name: String,
    /// List of active internal virtual channels (0-15) for this stop
    active_channels: Vec<u8>,
}

// NEW: Add ToSchema
#[derive(Deserialize, ToSchema)]
pub struct ChannelUpdateRequest {
    /// True to enable the stop for this channel, False to disable
    active: bool,
}

#[derive(Serialize, Clone, ToSchema)]
pub struct OrganInfoResponse {
    /// The name of the loaded organ definition
    name: String,
}

// --- Shared State ---

struct ApiData {
    app_state: Arc<Mutex<AppState>>,
    audio_tx: Sender<AppMessage>,
}

// --- OpenAPI Documentation Struct ---

#[derive(OpenApi)]
#[openapi(
    paths(
        get_stops, 
        update_stop_channel,
        get_organ_info
    ),
    components(
        schemas(StopStatusResponse, ChannelUpdateRequest, OrganInfoResponse)
    ),
    tags(
        (name = "Rusty Pipes API", description = "Control endpoints for the virtual organ")
    )
)]
struct ApiDoc;

// --- Handlers ---

#[utoipa::path(
    get,
    path = "/organ",
    tag = "General",
    responses(
        (status = 200, description = "Organ information", body = OrganInfoResponse)
    )
)]
async fn get_organ_info(data: web::Data<ApiData>) -> impl Responder {
    let state = data.app_state.lock().unwrap();
    
    let response = OrganInfoResponse {
        name: state.organ.name.clone(),
    };
    
    HttpResponse::Ok().json(response)
}

/// Returns a JSON list of all stops and their currently enabled virtual channels.
#[utoipa::path(
    get,
    path = "/stops",
    tag = "Stops",
    responses(
        (status = 200, description = "List of all stops and their active channels", body = Vec<StopStatusResponse>)
    )
)]
async fn get_stops(data: web::Data<ApiData>) -> impl Responder {
    let state = data.app_state.lock().unwrap();
    
    let mut response_list = Vec::with_capacity(state.organ.stops.len());
    
    for (i, stop) in state.organ.stops.iter().enumerate() {
        let mut active_channels = state.stop_channels.get(&i)
            .map(|set| set.iter().cloned().collect::<Vec<u8>>())
            .unwrap_or_default();
        active_channels.sort();

        response_list.push(StopStatusResponse {
            index: i,
            name: stop.name.clone(),
            active_channels,
        });
    }
    
    HttpResponse::Ok().json(response_list)
}

/// Enables or disables a specific stop for a specific virtual MIDI channel.
#[utoipa::path(
    post,
    path = "/stops/{stop_id}/channels/{channel_id}",
    tag = "Stops",
    request_body = ChannelUpdateRequest,
    params(
        ("stop_id" = usize, Path, description = "Index of the stop"),
        ("channel_id" = u8, Path, description = "Virtual MIDI Channel (0-15)")
    ),
    responses(
        (status = 200, description = "Channel updated successfully"),
        (status = 400, description = "Invalid channel ID"),
        (status = 404, description = "Stop index not found"),
        (status = 500, description = "Internal application error")
    )
)]
async fn update_stop_channel(
    path: web::Path<(usize, u8)>,
    body: web::Json<ChannelUpdateRequest>,
    data: web::Data<ApiData>
) -> impl Responder {
    let (stop_index, channel_id) = path.into_inner();
    
    if channel_id > 15 {
        return HttpResponse::BadRequest().body("Channel ID must be between 0 and 15");
    }

    let mut state = data.app_state.lock().unwrap();

    if stop_index >= state.organ.stops.len() {
        return HttpResponse::NotFound().body(format!("Stop index {} not found", stop_index));
    }

    match state.set_stop_channel_state(stop_index, channel_id, body.active, &data.audio_tx) {
        Ok(_) => {
            let action = if body.active { "Enabled" } else { "Disabled" };
            state.add_midi_log(format!("API: {} Stop {} for Ch {}", action, stop_index, channel_id + 1));
            
            HttpResponse::Ok().json(serde_json::json!({
                "status": "success", 
                "stop_index": stop_index,
                "channel": channel_id,
                "active": body.active
            }))
        },
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to update state: {}", e))
        }
    }
}

/// Redirects the root path to the Swagger UI.
async fn index() -> impl Responder {
    HttpResponse::Found()
        .append_header(("Location", "/swagger-ui/"))
        .finish()
}

// --- Server Launcher ---

pub fn start_api_server(
    app_state: Arc<Mutex<AppState>>,
    audio_tx: Sender<AppMessage>,
    port: u16
) {
    std::thread::spawn(move || {
        let sys = actix_web::rt::System::new();
        
        let server_data = web::Data::new(ApiData {
            app_state,
            audio_tx,
        });

        let openapi = ApiDoc::openapi();

        let server = HttpServer::new(move || {
            App::new()
                .app_data(server_data.clone())
                .service(
                    SwaggerUi::new("/swagger-ui/{_:.*}")
                        .url("/api-docs/openapi.json", openapi.clone()),
                )
                .route("/", web::get().to(index))
                .route("/stops", web::get().to(get_stops))
                .route("/stops/{stop_id}/channels/{channel_id}", web::post().to(update_stop_channel))
                .route("/organ", web::get().to(get_organ_info))
        })
        .bind(("0.0.0.0", port));

        match server {
            Ok(srv) => {
                println!("REST API server listening on http://0.0.0.0:{}", port);
                println!("Swagger UI available at http://0.0.0.0:{}/swagger-ui/", port);
                if let Err(e) = sys.block_on(srv.run()) {
                    eprintln!("API Server Error: {}", e);
                }
            },
            Err(e) => eprintln!("Failed to bind API server to port {}: {}", port, e),
        }
    });
}