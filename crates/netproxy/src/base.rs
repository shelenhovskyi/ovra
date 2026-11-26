//! The base allowlists: what each provider's agent itself needs, from where
//! Ovra runs (ADR 0011). A session gets its own provider's.

use ovra_core::protocol::agents::AgentKind;

use crate::rules::Rule;

/// Every provider with a base list. A new provider goes here and in
/// [`rules`].
pub(crate) const PROVIDERS: [AgentKind; 1] = [AgentKind::Claude];

/// The base rules of `agent` for the environment `var` reads.
pub(crate) fn rules(agent: AgentKind, var: &dyn Fn(&str) -> Option<String>) -> Vec<Rule> {
    match agent {
        AgentKind::Claude => claude(var),
    }
}

/// Hosts a claude.ai or Anthropic API login needs: the API, sign-in and
/// token refresh (`code.claude.com/docs/en/network-config`). Telemetry
/// intake hosts are left out: anyone with an account there receives data.
const ANTHROPIC: [&str; 3] = ["api.anthropic.com", "claude.ai", "platform.claude.com"];

/// What `claude` needs: Anthropic's hosts, and the gateway and cloud
/// provider its variables name. Endpoints the user set in a `*_BASE_URL`
/// variable may be on this machine or the local network (a gateway); only
/// that exact host and port is let through there.
pub(crate) fn claude(var: &dyn Fn(&str) -> Option<String>) -> Vec<Rule> {
    let mut rules: Vec<Rule> = ANTHROPIC.iter().map(|h| Rule::named(h)).collect();
    let on = |name: &str| {
        var(name).is_some_and(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
    };
    let configured = |name: &str| var(name).as_deref().and_then(crate::url::Authority::parse);
    let mut add = |rule: Rule| {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    };
    if let Some(gateway) = configured("ANTHROPIC_BASE_URL") {
        add(Rule::configured(gateway));
    }
    let region = |names: &[&str], default: &str| {
        names
            .iter()
            .find_map(|n| var(n).filter(|r| label(r)))
            .unwrap_or_else(|| default.to_owned())
    };
    if on("CLAUDE_CODE_USE_BEDROCK") {
        match configured("ANTHROPIC_BEDROCK_BASE_URL") {
            Some(url) => add(Rule::configured(url)),
            None => {
                let region = region(&["AWS_REGION", "AWS_DEFAULT_REGION"], "us-east-1");
                add(Rule::named(&format!(
                    "bedrock-runtime.{region}.amazonaws.com"
                )));
            }
        }
    }
    if on("CLAUDE_CODE_USE_VERTEX") {
        match configured("ANTHROPIC_VERTEX_BASE_URL") {
            Some(url) => add(Rule::configured(url)),
            None => {
                let host = match region(&["CLOUD_ML_REGION"], "us-east5").as_str() {
                    "global" => "aiplatform.googleapis.com".to_owned(),
                    "eu" => "aiplatform.eu.rep.googleapis.com".to_owned(),
                    "us" => "aiplatform.us.rep.googleapis.com".to_owned(),
                    region => format!("{region}-aiplatform.googleapis.com"),
                };
                add(Rule::named(&host));
            }
        }
        // Google's token exchange for application default credentials.
        add(Rule::named("oauth2.googleapis.com"));
    }
    if on("CLAUDE_CODE_USE_FOUNDRY") {
        if let Some(url) = configured("ANTHROPIC_FOUNDRY_BASE_URL") {
            add(Rule::configured(url));
        } else if let Some(resource) = var("ANTHROPIC_FOUNDRY_RESOURCE").filter(|r| label(r)) {
            add(Rule::named(&format!(
                "{}.services.ai.azure.com",
                resource.to_ascii_lowercase()
            )));
        }
    }
    rules
}

/// One DNS label, so a variable cannot smuggle more into a host name.
fn label(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 63
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
