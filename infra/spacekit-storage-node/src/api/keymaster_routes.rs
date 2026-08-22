//! SKKM-1 object store: opaque blobs keyed by `object_id` for shard envelopes and sealed blobs.

use std::path::PathBuf;

use bytes::Bytes;
use warp::{http::Response, hyper::Body, Filter, Rejection};

fn object_path(data_dir: &PathBuf, object_id: &str) -> PathBuf {
    let id = object_id.trim_start_matches("0x");
    data_dir.join("keymaster").join("objects").join(id)
}

fn json_status(status: warp::http::StatusCode, value: serde_json::Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}

async fn handle_put(
    object_id: String,
    body: Bytes,
    _node_did: Option<String>,
    data_dir: Option<PathBuf>,
) -> Result<Response<Body>, Rejection> {
    let Some(dir) = data_dir else {
        return Ok(json_status(
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "error": "no data_dir" }),
        ));
    };
    let path = object_path(&dir, &object_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| warp::reject())?;
    }
    std::fs::write(&path, &body).map_err(|_| warp::reject())?;
    Ok(json_status(
        warp::http::StatusCode::NO_CONTENT,
        serde_json::json!({ "ok": true }),
    ))
}

async fn handle_get(
    object_id: String,
    _node_did: Option<String>,
    data_dir: Option<PathBuf>,
) -> Result<Response<Body>, Rejection> {
    let Some(dir) = data_dir else {
        return Ok(json_status(
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "error": "no data_dir" }),
        ));
    };
    let path = object_path(&dir, &object_id);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Response::builder()
            .status(warp::http::StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap()),
        Err(_) => Ok(json_status(
            warp::http::StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "not found" }),
        )),
    }
}

async fn handle_delete(
    object_id: String,
    _node_did: Option<String>,
    data_dir: Option<PathBuf>,
) -> Result<Response<Body>, Rejection> {
    let Some(dir) = data_dir else {
        return Ok(json_status(
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "error": "no data_dir" }),
        ));
    };
    let path = object_path(&dir, &object_id);
    let _ = std::fs::remove_file(path);
    Ok(json_status(
        warp::http::StatusCode::NO_CONTENT,
        serde_json::json!({ "ok": true }),
    ))
}

pub fn put_route(
    data_dir: Option<PathBuf>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_put = data_dir;
    warp::path!("v1" / "keymaster" / "objects" / String)
        .and(warp::put())
        .and(warp::body::bytes())
        .and(warp::header::optional::<String>("x-spacekit-node-did"))
        .and(warp::any().map(move || data_put.clone()))
        .and_then(handle_put)
}

pub fn get_route(
    data_dir: Option<PathBuf>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_get = data_dir;
    warp::path!("v1" / "keymaster" / "objects" / String)
        .and(warp::get())
        .and(warp::header::optional::<String>("x-spacekit-node-did"))
        .and(warp::any().map(move || data_get.clone()))
        .and_then(handle_get)
}

pub fn delete_route(
    data_dir: Option<PathBuf>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_del = data_dir;
    warp::path!("v1" / "keymaster" / "objects" / String)
        .and(warp::delete())
        .and(warp::header::optional::<String>("x-spacekit-node-did"))
        .and(warp::any().map(move || data_del.clone()))
        .and_then(handle_delete)
}
