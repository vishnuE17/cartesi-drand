use crate::utils::util::deserialize_obj;
use hyper::{Body, Response};
use serde::{Deserialize, Serialize};

pub mod server {
    use hyper::{Body, Response};
    use serde_json::{json, Value};

    use super::{parse_input_from_response, RollupInput};

    pub(crate) async fn send_finish(
        status: &str,
    ) -> Result<Response<Body>, Box<dyn std::error::Error>> {
        let server_addr = std::env::var("ROLLUP_HTTP_SERVER_URL").expect("Env is not set");
        println!("Sending finish to {}", &server_addr);
        let client = hyper::Client::new();
        let response = json!({"status" : status.clone()});
        let request = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(format!("{}/finish", &server_addr))
            .body(hyper::Body::from(response.to_string()))?;

        let response = client.request(request).await?;

        println!(
            "Received finish status {} from RollupServer",
            response.status()
        );
        Ok(response)
    }

    pub(crate) async fn send_finish_and_retrieve_input(status: &str) -> Option<RollupInput> {
        let response = send_finish(status)
            .await
            .map_err(|err| {
                eprintln!("Error {:?}", err);
                err
            })
            .ok()?;

        if response.status() == hyper::StatusCode::ACCEPTED {
            return None;
        }
        parse_input_from_response(response)
            .await
            .map_err(|err| {
                eprintln!("Error {:?}", err);
                err
            })
            .ok()
    }

    pub(crate) async fn send_report(
        report: Value,
    ) -> Result<&'static str, Box<dyn std::error::Error>> {
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
pub(crate) struct RollupInput {
    pub(crate) data: RollupInputData,
    pub(crate) request_type: String,
}

impl RollupInput {
    pub(crate) fn decoded_inspect(&self) -> String {
        let payload = self.data.payload.trim_start_matches("0x");
        let bytes: Vec<u8> = hex::decode(&payload).unwrap();
        let inspect_decoded = std::str::from_utf8(&bytes).unwrap();
        inspect_decoded.to_string()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RollupInputData {
    pub(crate) payload: String,
    pub(crate) metadata: Option<RollupInputDataMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RollupInputDataMetadata {
    pub(crate) block_number: u128,
    pub(crate) epoch_index: u128,
    pub(crate) input_index: u128,
    pub(crate) msg_sender: String,
    pub(crate) timestamp: u64,
}

pub(crate) async fn parse_input_from_response(
    response: Response<Body>,
) -> Result<RollupInput, Box<dyn std::error::Error>> {
    let body = hyper::body::to_bytes(response).await?;
    let utf = std::str::from_utf8(&body)?;
    let result_deserialization = serde_json::from_str::<RollupInput>(utf)?;
    Ok(result_deserialization)
}

pub(crate) fn has_input_inside_input(body: &String) -> bool {
    let result_deserialization = serde_json::from_str::<RollupInput>(body);
    let rollup_input = match result_deserialization {
        Ok(input) => input,
        Err(_) => return false,
    };
    let json = rollup_input.data.payload.trim_start_matches("0x");
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
