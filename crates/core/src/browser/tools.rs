//! What the `browser_*` IDE tools do, for `ovra-mcp-server` (task B.C).
//! Every role gets them, each for the caller's own session's pages only.
//! What they return comes from the page: the tools wrap it as data.

use super::{Result, page_not_found, pages, picks};
use crate::Core;
use crate::protocol::browser::{PageId, PageInfo};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{ConsoleEntry, ElementRef, Inspected, NetworkEntry, PageAction, ToolCaller};

impl Core {
    /// `browser_open`: a new page of the caller's browser.
    pub async fn browser_open(
        &self,
        caller: &ToolCaller,
        url: String,
        size: Option<(u32, u32)>,
    ) -> Result<PageInfo> {
        super::open(self, &caller.session, url, size).await
    }

    /// The caller's open page, as it is now.
    pub fn browser_page(&self, caller: &ToolCaller, page: &PageId) -> Result<PageInfo> {
        pages(self, &caller.session)
            .into_iter()
            .find(|p| p.id == *page)
            .ok_or_else(|| page_not_found(page))
    }

    /// `browser_navigate`, `browser_click`, `browser_type`,
    /// `browser_press_key` and `browser_wait_for`; answers the page after.
    pub async fn browser_act(
        &self,
        caller: &ToolCaller,
        page: &PageId,
        action: PageAction,
    ) -> Result<PageInfo> {
        self.browser_page(caller, page)?;
        self.browser_state().browser.act(page, action).await?;
        self.browser_page(caller, page)
    }

    /// `browser_snapshot`: the accessibility tree as text with refs.
    pub async fn browser_snapshot(
        &self,
        caller: &ToolCaller,
        page: &PageId,
        target: Option<String>,
        depth: Option<u32>,
    ) -> Result<String> {
        self.browser_page(caller, page)?;
        let browser = &self.browser_state().browser;
        browser.snapshot(page, target, depth).await
    }

    /// `browser_console`: up to `limit` messages numbered above `since`.
    pub fn browser_console(
        &self,
        caller: &ToolCaller,
        page: &PageId,
        since: u64,
        limit: usize,
    ) -> Result<Vec<ConsoleEntry>> {
        self.browser_page(caller, page)?;
        self.browser_state().browser.console(page, since, limit)
    }

    /// `browser_network`: up to `limit` requests numbered above `since`.
    pub fn browser_network(
        &self,
        caller: &ToolCaller,
        page: &PageId,
        since: u64,
        limit: usize,
    ) -> Result<Vec<NetworkEntry>> {
        self.browser_page(caller, page)?;
        self.browser_state().browser.network(page, since, limit)
    }

    /// `browser_screenshot`: a PNG of the viewport, the page or a ref.
    pub async fn browser_screenshot(
        &self,
        caller: &ToolCaller,
        page: &PageId,
        target: Option<String>,
        full_page: bool,
    ) -> Result<Vec<u8>> {
        self.browser_page(caller, page)?;
        let browser = &self.browser_state().browser;
        browser.screenshot(page, target, full_page).await
    }

    /// `browser_inspect`: a ref of `page`, or a pick the caller's messages
    /// carried. A pick whose page closed or changed answers what the person
    /// sent, without an image (`png` empty).
    pub async fn browser_inspect(
        &self,
        caller: &ToolCaller,
        page: Option<PageId>,
        element: ElementRef,
    ) -> Result<Inspected> {
        let sent = match &element {
            ElementRef::Pick(id) => picks::find_sent(self, &caller.session, id),
            ElementRef::Ref(_) => None,
        };
        let page = page
            .or_else(|| sent.as_ref().map(|(p, _)| p.clone()))
            .ok_or_else(|| ProtocolError::new(ErrorCode::BadRequest, "name the page_id"))?;
        let live = match self.browser_page(caller, &page) {
            Ok(_) => self.browser_state().browser.inspect(&page, element).await,
            Err(e) => Err(e),
        };
        match (live, sent) {
            (Ok(mut found), Some((_, pick))) => {
                found.element.note = pick.note;
                Ok(found)
            }
            (Ok(found), None) => Ok(found),
            (Err(_), Some((_, pick))) => Ok(Inspected {
                element: pick,
                png: Vec::new(),
            }),
            (Err(e), None) => Err(e),
        }
    }

    /// `browser_close`: closes one of the caller's pages.
    pub async fn browser_close(&self, caller: &ToolCaller, page: &PageId) -> Result<()> {
        self.browser_page(caller, page)?;
        self.browser_state().browser.close(page).await
    }
}
