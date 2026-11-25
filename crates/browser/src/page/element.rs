//! Elements of a page: their boxes, their pick payloads, screenshots of
//! them. Read in Ovra's isolated world, never the page's own.

use base64::Engine as _;
use ovra_core::protocol::browser::Pick;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{ElementRef, Result};
use serde_json::{Value, json};

use super::{PICKS_KEPT, Page, error, lock};
use crate::pick;

impl Page {
    /// The middle of the element, scrolled into view, in viewport pixels.
    pub(super) async fn center(&self, backend: i64) -> Result<(i32, i32)> {
        let (x, y, width, height) = self.rect(backend).await?;
        Ok((
            (x + width / 2.0).round() as i32,
            (y + height / 2.0).round() as i32,
        ))
    }

    /// The element's box (x, y, width, height) after scrolling it into view.
    async fn rect(&self, backend: i64) -> Result<(f64, f64, f64, f64)> {
        let node = json!({ "backendNodeId": backend });
        self.call("DOM.scrollIntoViewIfNeeded", node.clone())
            .await
            .map_err(stale)?;
        let quads = self
            .call("DOM.getContentQuads", node)
            .await
            .map_err(stale)?;
        let quad: Vec<f64> = quads["quads"][0]
            .as_array()
            .map(|q| q.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if quad.len() != 8 {
            return Err(error(ErrorCode::Conflict, "the element is not visible"));
        }
        let xs = [quad[0], quad[2], quad[4], quad[6]];
        let ys = [quad[1], quad[3], quad[5], quad[7]];
        let (x0, x1) = (
            xs.iter().copied().fold(f64::MAX, f64::min),
            xs.iter().copied().fold(f64::MIN, f64::max),
        );
        let (y0, y1) = (
            ys.iter().copied().fold(f64::MAX, f64::min),
            ys.iter().copied().fold(f64::MIN, f64::max),
        );
        Ok((x0, y0, x1 - x0, y1 - y0))
    }

    /// Ovra's isolated world in the main frame, made when first needed.
    pub(super) async fn world(&self) -> Result<i64> {
        if let Some(world) = lock(&self.state).world {
            return Ok(world);
        }
        let made = self
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId": self.target, "worldName": "ovra"}),
            )
            .await?;
        let world = made["executionContextId"]
            .as_i64()
            .ok_or_else(|| error(ErrorCode::Internal, "no isolated world"))?;
        lock(&self.state).world = Some(world);
        Ok(world)
    }

    /// Calls `function` on the object `id` in Ovra's world.
    async fn on(&self, id: &Value, function: &str, args: Value, by_value: bool) -> Result<Value> {
        let params = json!({"objectId": id, "functionDeclaration": function, "arguments": args, "returnByValue": by_value});
        let answer = self.call("Runtime.callFunctionOn", params).await?;
        if answer.get("exceptionDetails").is_some() {
            return Err(error(ErrorCode::Internal, "reading the element failed"));
        }
        Ok(answer["result"].clone())
    }

    /// The pick payload of `backend`, or its ancestor `up` levels above.
    async fn element(&self, backend: i64, up: u32, pick_id: String) -> Result<(i64, Pick)> {
        let world = self.world().await?;
        let resolved = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId": backend, "executionContextId": world}),
            )
            .await
            .map_err(stale)?;
        let element = self
            .on(
                &resolved["object"]["objectId"],
                pick::ANCESTOR,
                json!([{ "value": up }]),
                false,
            )
            .await?;
        if element["subtype"] == json!("null") || element.get("objectId").is_none() {
            return Err(error(ErrorCode::NotFound, "no element there"));
        }
        let id = &element["objectId"];
        let described = self
            .call("DOM.describeNode", json!({ "objectId": id }))
            .await?;
        let backend = described["node"]["backendNodeId"]
            .as_i64()
            .unwrap_or(backend);
        let read = self.on(id, &pick::read_function(), json!([]), true).await?;
        let read = &read["value"];
        let ax = self
            .call(
                "Accessibility.getPartialAXTree",
                json!({"backendNodeId": backend, "fetchRelatives": false}),
            )
            .await
            .unwrap_or_default();
        let locator = pick::locator(&ax, read["tag"].as_str().unwrap_or_default());
        let mut state = lock(&self.state);
        let found = pick::build(read, pick_id.clone(), state.url.clone(), locator);
        state.picks.retain(|(id, _)| *id != pick_id);
        if state.picks.len() == PICKS_KEPT {
            state.picks.pop_front();
        }
        state.picks.push_back((pick_id, backend));
        Ok((backend, found))
    }

    pub(crate) async fn pick_at(&self, x: i32, y: i32, up: u32, pick_id: String) -> Result<Pick> {
        let at = json!({"x": x, "y": y, "includeUserAgentShadowDOM": false, "ignorePointerEventsNone": true});
        let hit = self
            .call("DOM.getNodeForLocation", at)
            .await
            .map_err(|_| error(ErrorCode::NotFound, "no element there"))?;
        let backend = hit["backendNodeId"]
            .as_i64()
            .ok_or_else(|| error(ErrorCode::NotFound, "no element there"))?;
        Ok(self.element(backend, up, pick_id).await?.1)
    }

    /// The element of a ref or a pick, with a cropped PNG of it.
    pub(crate) async fn inspect(
        &self,
        element: ElementRef,
        new_id: String,
    ) -> Result<(Pick, Vec<u8>)> {
        let (backend, pick_id) = match element {
            ElementRef::Ref(target) => (self.node(&target)?, new_id),
            ElementRef::Pick(id) => {
                let found = lock(&self.state)
                    .picks
                    .iter()
                    .find(|(p, _)| *p == id)
                    .map(|(_, b)| *b);
                let backend = found.ok_or_else(|| {
                    error(
                        ErrorCode::NotFound,
                        format!("pick {id} is gone: pick it again"),
                    )
                })?;
                (backend, id)
            }
        };
        let png = self.capture(Some(backend), false).await?;
        let (_, found) = self.element(backend, 0, pick_id).await?;
        Ok((found, png))
    }

    pub(crate) async fn screenshot(
        &self,
        target: Option<String>,
        full_page: bool,
    ) -> Result<Vec<u8>> {
        let backend = target.map(|t| self.node(&t)).transpose()?;
        self.capture(backend, full_page).await
    }

    async fn capture(&self, backend: Option<i64>, full_page: bool) -> Result<Vec<u8>> {
        let mut params = json!({ "format": "png" });
        if let Some(backend) = backend {
            let (x, y, width, height) = self.rect(backend).await?;
            let metrics = self.call("Page.getLayoutMetrics", json!({})).await?;
            let view = &metrics["cssVisualViewport"];
            let (left, top) = (
                view["pageX"].as_f64().unwrap_or_default(),
                view["pageY"].as_f64().unwrap_or_default(),
            );
            params["clip"] = json!({"x": x + left, "y": y + top, "width": width.max(1.0), "height": height.max(1.0), "scale": 1});
        } else if full_page {
            let metrics = self.call("Page.getLayoutMetrics", json!({})).await?;
            let size = &metrics["cssContentSize"];
            params["clip"] = json!({"x": 0, "y": 0, "width": size["width"], "height": size["height"], "scale": 1});
            params["captureBeyondViewport"] = json!(true);
        }
        let shot = self.call("Page.captureScreenshot", params).await?;
        base64::engine::general_purpose::STANDARD
            .decode(shot["data"].as_str().unwrap_or_default())
            .map_err(|_| error(ErrorCode::Internal, "the screenshot was not base64"))
    }
}

/// A node CDP no longer knows: the page changed since the snapshot.
pub(super) fn stale(e: ProtocolError) -> ProtocolError {
    if e.message.contains("No node") || e.message.contains("not found") {
        error(
            ErrorCode::Conflict,
            "the element is gone: take a new snapshot",
        )
    } else {
        e
    }
}
