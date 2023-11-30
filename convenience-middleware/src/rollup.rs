use crate::{models::models::Item, utils::util::deserialize_obj};
use hyper::{Body, Response};
use serde::{Deserialize, Serialize};

pub mod server {
    use hyper::{Body, Response};
    use log::{error, info};
    use serde_json::{json, Value};

    use super::{parse_input_from_response, RollupInput};

    pub async fn send_finish(status: &str) -> Result<Response<Body>, Box<dyn std::error::Error>> {
        let server_addr = std::env::var("ROLLUP_HTTP_SERVER_URL").expect("Env is not set");
        info!("Sending finish to {}", &server_addr);
        let client = hyper::Client::new();
        let response = json!({"status" : status});
        let request = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(format!("{}/finish", &server_addr))
            .body(hyper::Body::from(response.to_string()))?;

        let response = client.request(request).await?;

        info!(
            "Received finish status {} from RollupServer",
            response.status()
        );
        Ok(response)
    }

    pub async fn send_finish_and_retrieve_input(status: &str) -> Option<RollupInput> {
        let response = send_finish(status)
            .await
            .map_err(|err| {
                error!("Error {:?}", err);
                err
            })
            .ok()?;

        if response.status() == hyper::StatusCode::ACCEPTED {
            return None;
        }
        parse_input_from_response(response)
            .await
            .map_err(|err| {
                error!("Error {:?}", err);
                err
            })
            .ok()
    }

    pub async fn send_report(report: Value) -> Result<&'static str, Box<dyn std::error::Error>> {
        let server_addr =
            std::env::var("ROLLUP_HTTP_SERVER_URL").expect("ROLLUP_HTTP_SERVER_URL is not set");
        let client = hyper::Client::new();
        let req = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(format!("{}/report", server_addr))
            .body(hyper::Body::from(report.to_string()))?;

        let _ = client.request(req).await?;
        Ok("accept")
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollupInput {
    pub data: RollupInputData,
    pub request_type: String,
}

impl TryFrom<Item> for RollupInput {
    type Error = serde_json::Error;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        serde_json::from_str(&item.request)
    }
}

impl RollupInput {
    pub fn decoded_inspect(&self) -> String {
        let payload = self.data.payload.trim_start_matches("0x");
        let bytes: Vec<u8> = hex::decode(&payload).unwrap();
        let inspect_decoded = std::str::from_utf8(&bytes).unwrap();
        inspect_decoded.to_string()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollupInputData {
    pub payload: String,
    pub metadata: Option<RollupInputDataMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RollupInputDataMetadata {
    pub block_number: u128,
    pub epoch_index: u128,
    pub input_index: u128,
    pub msg_sender: String,
    pub timestamp: u64,
}

pub async fn parse_input_from_response(
    response: Response<Body>,
) -> Result<RollupInput, Box<dyn std::error::Error>> {
    let body = hyper::body::to_bytes(response).await?;
    let utf = std::str::from_utf8(&body)?;
    let result_deserialization = serde_json::from_str::<RollupInput>(utf)?;
    Ok(result_deserialization)
}

pub fn has_input_inside_input(input: &RollupInput) -> bool {
    let json = input.data.payload.trim_start_matches("0x");
    let json = hex::decode(json);
    let json = match json {
        Ok(json) => json,
        Err(_) => return false,
    };
    let json = std::str::from_utf8(&json).unwrap();
    let value = deserialize_obj(json);
    let value = match value {
        Some(json) => json,
        None => return false,
    };
    value.contains_key("input")
}
