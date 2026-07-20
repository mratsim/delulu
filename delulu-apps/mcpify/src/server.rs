use crate::openapi::{OpenApiSpec, Operation, ParameterLocation};
use crate::proxy::ProxyClient;
use anyhow::Result;
use futures::FutureExt;
use rmcp::handler::server::tool::{ToolCallContext, DynCallToolHandler};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{CallToolRequestParam, CallToolResult, Tool};
use rmcp::service::RequestContext;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct McpifyServer {
    base_url: String,
    proxy: Arc<ProxyClient>,
    tools: Vec<ToolEntry>,
}

#[derive(Clone)]
struct ToolEntry {
    name: String,
    tool: Tool,
    handler: Arc<DynCallToolHandler<McpifyServer>>,
}

impl McpifyServer {
    pub fn from_openapi(spec: &OpenApiSpec) -> Result<Self> {
        let base_url = spec
            .base_url()
            .ok_or_else(|| anyhow::anyhow!("No servers defined in OpenAPI spec"))?
            .to_string();

        let proxy = Arc::new(ProxyClient::new()?);

        let mut tools = Vec::new();

        for (path, path_item) in spec.paths.iter() {
            if let Some(op) = &path_item.get {
                if let Some(entry) = Self::build_tool(&base_url, path, op, proxy.clone())? {
                    tools.push(entry);
                }
            }
            if let Some(op) = &path_item.post {
                if let Some(entry) = Self::build_tool(&base_url, path, op, proxy.clone())? {
                    tools.push(entry);
                }
            }
        }

        tracing::info!("Registered {} tools", tools.len());

        Ok(Self {
            base_url,
            proxy,
            tools,
        })
    }

    fn build_tool(
        base_url: &str,
        path: &str,
        op: &Operation,
        proxy: Arc<ProxyClient>,
    ) -> Result<Option<ToolEntry>> {
        let operation_id = match &op.operation_id {
            Some(id) => id.clone(),
            None => return Ok(None),
        };

        let description = op
            .description
            .clone()
            .or_else(|| op.summary.clone())
            .unwrap_or_else(|| operation_id.clone());

        let input_schema = build_input_schema(op);

        let tool = Tool::new(
            operation_id.clone(),
            description,
            rmcp::model::object(input_schema),
        );

        let base_url = base_url.to_string();
        let path = path.to_string();

        // Extract path parameter names from the operation definition
        let path_param_names: Vec<String> = op
            .parameters
            .iter()
            .filter(|p| matches!(p.location, ParameterLocation::Path))
            .map(|p| p.name.clone())
            .collect();

        let handler: Arc<DynCallToolHandler<McpifyServer>> = Arc::new(
            move |ctx: ToolCallContext<'_, McpifyServer>| {
                let base_url = base_url.clone();
                let path = path.clone();
                let path_param_names = path_param_names.clone();
                let proxy = proxy.clone();
                async move {
                    let params = ctx.arguments.unwrap_or_default();
                    let params_map: HashMap<String, Value> = params
                        .into_iter()
                        .map(|(k, v)| (k, v))
                        .collect();

                    let result = proxy
                        .get(&base_url, &path, params_map, &path_param_names)
                        .await;

                    let json = serde_json::to_value(&result).unwrap_or(json!({"error": "Serialization failed"}));
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        serde_json::to_string(&json).unwrap_or_default(),
                    )]))
                }
                .boxed()
            },
        );

        Ok(Some(ToolEntry {
            name: operation_id,
            tool,
            handler,
        }))
    }

    fn find_tool(&self, name: &str) -> Option<&ToolEntry> {
        self.tools.iter().find(|t| t.name == name)
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.tools.iter().map(|t| t.tool.clone()).collect()
    }
}

fn build_input_schema(op: &Operation) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for param in &op.parameters {
        let schema = match &param.schema {
            Some(s) => param_schema_to_json_schema(s),
            None => json!({ "type": "string" }),
        };

        let mut schema = schema;
        if let Some(obj) = schema.as_object_mut() {
            if let Some(desc) = &param.description {
                obj.insert("description".to_string(), Value::String(desc.clone()));
            }
            properties.insert(param.name.clone(), Value::Object(obj.clone()));
        } else {
            properties.insert(param.name.clone(), schema);
        }

        if param.required {
            required.push(param.name.clone());
        }
    }

    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String("object".to_string()));
    obj.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        obj.insert("required".to_string(), Value::Array(
            required.into_iter().map(Value::String).collect()
        ));
    }

    Value::Object(obj)
}

fn param_schema_to_json_schema(schema: &crate::openapi::ParameterSchema) -> Value {
    let mut obj = Map::new();

    if let Some(t) = &schema.param_type {
        obj.insert("type".to_string(), Value::String(t.clone()));
    }

    if let Some(f) = &schema.format {
        obj.insert("format".to_string(), Value::String(f.clone()));
    }

    if let Some(e) = &schema.enum_values {
        obj.insert("enum".to_string(), Value::Array(e.clone()));
    }

    if let Some(d) = &schema.default {
        obj.insert("default".to_string(), d.clone());
    }

    if let Some(desc) = &schema.description {
        obj.insert("description".to_string(), Value::String(desc.clone()));
    }

    if let Some(ex) = &schema.examples {
        obj.insert("example".to_string(), ex.clone());
    }

    if schema.nullable == Some(true) {
        obj.insert("nullable".to_string(), Value::Bool(true));
    }

    Value::Object(obj)
}

impl ServerHandler for McpifyServer {
    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        let tools = self.list_tools();
        tracing::debug!("list_tools called, returning {} tools", tools.len());
        async move { Ok(rmcp::model::ListToolsResult::with_all_items(tools)) }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        let name = request.name.to_string();
        
        async move {
            let tool = self
                .find_tool(&name)
                .ok_or_else(|| rmcp::ErrorData::invalid_params("tool not found", None))?;

            let call_ctx = ToolCallContext::new(self, request, context);
            let result = (tool.handler)(call_ctx).await;
            
            result
        }
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2025_03_26,
            capabilities: rmcp::model::ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability::default()),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation::from_build_env(),
            instructions: None,
        }
    }
}
