#![allow(unused)]
use golem_common::base_model::agent::{
    UnstructuredBinaryExtensions, UnstructuredTextExtensions,
};
use golem_wasm::{FromValueAndType, IntoValueAndType};
pub struct ProjectionCursorAgent {
    constructor_parameters: golem_client::model::UntypedJsonDataValue,
    phantom_id: Option<uuid::Uuid>,
    agent_id: golem_common::model::AgentId,
}
impl std::fmt::Debug for ProjectionCursorAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ProjectionCursorAgent))
            .field("constructor_parameters", &self.constructor_parameters)
            .field("phantom_id", &self.phantom_id)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}
impl ProjectionCursorAgent {
    pub async fn get(
        account_id: String,
        projection_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() }),
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : projection_id.into_value_and_type() })
            ],
        });
        let constructor_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_constructor_parameters
            .into();
        Self::__create(constructor_parameters, None, vec![]).await
    }
    pub async fn get_phantom(
        uuid: uuid::Uuid,
        account_id: String,
        projection_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() }),
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : projection_id.into_value_and_type() })
            ],
        });
        let constructor_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_constructor_parameters
            .into();
        Self::__create(constructor_parameters, Some(uuid), vec![]).await
    }
    pub async fn new_phantom(
        account_id: String,
        projection_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() }),
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : projection_id.into_value_and_type() })
            ],
        });
        let constructor_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_constructor_parameters
            .into();
        Self::__create(constructor_parameters, Some(uuid::Uuid::new_v4()), vec![]).await
    }
    /// Returns the agent's identity, containing the component ID and agent name.
    pub fn agent_id(&self) -> &golem_common::model::AgentId {
        &self.agent_id
    }
    /// Returns the configured worker service URL.
    pub fn worker_service_url() -> reqwest::Url {
        CONFIG.get().expect("Configuration has not been set").server.url()
    }
    /// Returns the configured authentication token.
    pub fn auth_token() -> golem_client::Security {
        CONFIG.get().expect("Configuration has not been set").server.token()
    }
    async fn __create(
        constructor_parameters: golem_client::model::UntypedJsonDataValue,
        phantom_id: Option<uuid::Uuid>,
        agent_config: Vec<golem_client::model::AgentConfigEntryDto>,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let config = CONFIG.get().expect("Configuration has not been set");
        let client = reqwest_middleware::ClientWithMiddleware::from(
            reqwest::Client::builder().build().unwrap(),
        );
        let context = golem_client::Context {
            client,
            base_url: config.server.url(),
            security_token: config.server.token(),
        };
        let api_client = golem_client::api::AgentClientLive {
            context,
        };
        let response = golem_client::api::AgentClient::create_agent(
                &api_client,
                &golem_client::model::CreateAgentRequest {
                    app_name: config.app_name.to_string(),
                    env_name: config.env_name.to_string(),
                    agent_type_name: "ProjectionCursorAgent".to_string(),
                    parameters: constructor_parameters.clone(),
                    phantom_id,
                    config: Some(agent_config),
                },
            )
            .await?;
        Ok(Self {
            constructor_parameters,
            phantom_id,
            agent_id: response.agent_id,
        })
    }
    pub async fn advance(
        &self,
        input: AdvanceProjectionCursorInput,
    ) -> Result<
        Result<ProjectionCursorReceipt, ProjectionCursorError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__advance(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_advance(
        &self,
        input: AdvanceProjectionCursorInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__advance(golem_client::model::AgentInvocationMode::Schedule, None, input)
            .await?;
        Ok(())
    }
    pub async fn schedule_advance(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: AdvanceProjectionCursorInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__advance(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __advance(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: AdvanceProjectionCursorInput,
    ) -> Result<
        Option<Result<ProjectionCursorReceipt, ProjectionCursorError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : input.into_value_and_type() })
            ],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self.invoke("advance", method_parameters, mode, when).await?;
        if let Some(untyped_data_value) = response {
            let typed_data_value = golem_common::model::agent::DataValue::try_from_untyped_json(
                    untyped_data_value,
                    golem_common::model::agent::DataSchema::Tuple(golem_common::model::agent::NamedElementSchemas {
                        elements: vec![
                            golem_common::model::agent::NamedElementSchema { name :
                            "return_value".to_string(), schema :
                            golem_common::model::agent::ElementSchema::ComponentModel(golem_common::model::agent::ComponentModelElementSchema
                            { element_type :
                            golem_wasm::analysis::AnalysedType::Result(golem_wasm::analysis::TypeResult
                            { name : None.clone(), owner : None.clone(), ok :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ProjectionCursorReceipt".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "cursor"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("ProjectionCursorError".to_string()).clone(),
                            owner : None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "InvalidIdentity".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidIdentity".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EmptyEventIdentity".to_string(), typ : None, },
                            golem_wasm::analysis::NameOptionTypePair { name :
                            "NonContiguousAdvance".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NonContiguousAdvance".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "expected"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "proposed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CursorConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CursorConflict".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "expected"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "actual"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ReplayIdentityConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ReplayIdentityConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "cursor"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "existing_event_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_event_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }], },))), },), },), }
                        ],
                    }),
                )
                .map_err(|err| golem_client::bridge::ClientError::InvocationFailed {
                    message: format!("Failed to decode result value: {err}"),
                })?;
            match typed_data_value {
                golem_common::model::agent::DataValue::Tuple(element_values) => {
                    match element_values.elements.get(0) {
                        Some(
                            golem_common::model::agent::ElementValue::ComponentModel(
                                golem_common::model::agent::ComponentModelElementValue {
                                    value: vnt,
                                },
                            ),
                        ) => {
                            Ok(
                                Some(
                                    <Result<
                                        ProjectionCursorReceipt,
                                        ProjectionCursorError,
                                    >>::from_value_and_type(vnt.clone())
                                        .map_err(|err| golem_client::bridge::ClientError::InvocationFailed {
                                            message: format!("Failed to decode result value: {err}"),
                                        })?,
                                ),
                            )
                        }
                        _ => {
                            Err(golem_client::bridge::ClientError::InvocationFailed {
                                message: format!("Failed to decode result value"),
                            })?
                        }
                    }
                }
                _ => {
                    Err(golem_client::bridge::ClientError::InvocationFailed {
                        message: format!("Failed to decode result value"),
                    })?
                }
            }
        } else {
            Ok(None)
        }
    }
    pub async fn status(
        &self,
    ) -> Result<
        Result<ProjectionCursorStatus, ProjectionCursorError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__status(golem_client::model::AgentInvocationMode::Await, None)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_status(&self) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__status(golem_client::model::AgentInvocationMode::Schedule, None)
            .await?;
        Ok(())
    }
    pub async fn schedule_status(
        &self,
        when: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__status(golem_client::model::AgentInvocationMode::Schedule, Some(when))
            .await?;
        Ok(())
    }
    async fn __status(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<
        Option<Result<ProjectionCursorStatus, ProjectionCursorError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self.invoke("status", method_parameters, mode, when).await?;
        if let Some(untyped_data_value) = response {
            let typed_data_value = golem_common::model::agent::DataValue::try_from_untyped_json(
                    untyped_data_value,
                    golem_common::model::agent::DataSchema::Tuple(golem_common::model::agent::NamedElementSchemas {
                        elements: vec![
                            golem_common::model::agent::NamedElementSchema { name :
                            "return_value".to_string(), schema :
                            golem_common::model::agent::ElementSchema::ComponentModel(golem_common::model::agent::ComponentModelElementSchema
                            { element_type :
                            golem_wasm::analysis::AnalysedType::Result(golem_wasm::analysis::TypeResult
                            { name : None.clone(), owner : None.clone(), ok :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ProjectionCursorStatus".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "account_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "projection_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "cursor"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "last_event_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("ProjectionCursorError".to_string()).clone(),
                            owner : None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "InvalidIdentity".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidIdentity".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EmptyEventIdentity".to_string(), typ : None, },
                            golem_wasm::analysis::NameOptionTypePair { name :
                            "NonContiguousAdvance".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NonContiguousAdvance".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "expected"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "proposed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CursorConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CursorConflict".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "expected"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "actual"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ReplayIdentityConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ReplayIdentityConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "cursor"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "existing_event_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_event_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }], },))), },), },), }
                        ],
                    }),
                )
                .map_err(|err| golem_client::bridge::ClientError::InvocationFailed {
                    message: format!("Failed to decode result value: {err}"),
                })?;
            match typed_data_value {
                golem_common::model::agent::DataValue::Tuple(element_values) => {
                    match element_values.elements.get(0) {
                        Some(
                            golem_common::model::agent::ElementValue::ComponentModel(
                                golem_common::model::agent::ComponentModelElementValue {
                                    value: vnt,
                                },
                            ),
                        ) => {
                            Ok(
                                Some(
                                    <Result<
                                        ProjectionCursorStatus,
                                        ProjectionCursorError,
                                    >>::from_value_and_type(vnt.clone())
                                        .map_err(|err| golem_client::bridge::ClientError::InvocationFailed {
                                            message: format!("Failed to decode result value: {err}"),
                                        })?,
                                ),
                            )
                        }
                        _ => {
                            Err(golem_client::bridge::ClientError::InvocationFailed {
                                message: format!("Failed to decode result value"),
                            })?
                        }
                    }
                }
                _ => {
                    Err(golem_client::bridge::ClientError::InvocationFailed {
                        message: format!("Failed to decode result value"),
                    })?
                }
            }
        } else {
            Ok(None)
        }
    }
    async fn invoke(
        &self,
        method_name: &str,
        method_parameters: golem_client::model::UntypedJsonDataValue,
        mode: golem_client::model::AgentInvocationMode,
        schedule_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<
        Option<golem_client::model::UntypedJsonDataValue>,
        golem_client::bridge::ClientError,
    > {
        let config = CONFIG.get().expect("Configuration has not been set");
        let client = reqwest_middleware::ClientWithMiddleware::from(
            reqwest::Client::builder().build().unwrap(),
        );
        let context = golem_client::Context {
            client,
            base_url: config.server.url(),
            security_token: config.server.token(),
        };
        let client = golem_client::api::AgentClientLive {
            context,
        };
        let response = golem_client::api::AgentClient::invoke_agent(
                &client,
                None,
                &golem_client::model::AgentInvocationRequest {
                    app_name: config.app_name.to_string(),
                    env_name: config.env_name.to_string(),
                    agent_type_name: "ProjectionCursorAgent".to_string(),
                    parameters: self.constructor_parameters.clone(),
                    phantom_id: self.phantom_id.clone(),
                    method_name: method_name.to_string(),
                    method_parameters,
                    mode,
                    schedule_at,
                    idempotency_key: None,
                    deployment_revision: None,
                    owner_account_email: None,
                },
            )
            .await?;
        Ok(response.result)
    }
}
static CONFIG: std::sync::OnceLock<golem_client::bridge::Configuration> = std::sync::OnceLock::new();
pub fn configure(
    server: golem_client::bridge::GolemServer,
    app_name: &str,
    env_name: &str,
) {
    CONFIG
        .set(golem_client::bridge::Configuration {
            app_name: app_name.to_string(),
            env_name: env_name.to_string(),
            server,
        })
        .map_err(|_| ())
        .expect("Configuration has already been set");
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectionCursorReceipt {
    pub cursor: u64,
    pub event_id: String,
    pub replayed: bool,
}
impl golem_wasm::IntoValue for ProjectionCursorReceipt {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.cursor.into_value(), self.event_id.into_value(), self.replayed
                .into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("event_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("replayed", < bool as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ProjectionCursorReceipt).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ProjectionCursorReceipt {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let event_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let replayed = <bool as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { cursor, event_id, replayed })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 3usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvalidIdentity {
    pub detail: String,
}
impl golem_wasm::IntoValue for InvalidIdentity {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(InvalidIdentity).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for InvalidIdentity {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 1usize => {
                let detail = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { detail })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 1usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectionCursorStatus {
    pub account_id: String,
    pub projection_id: String,
    pub cursor: u64,
    pub last_event_id: Option<String>,
}
impl golem_wasm::IntoValue for ProjectionCursorStatus {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.account_id.into_value(), self.projection_id.into_value(), self
                .cursor.into_value(), self.last_event_id.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("account_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("projection_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("last_event_id", < Option < String > as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(ProjectionCursorStatus).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ProjectionCursorStatus {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let account_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let projection_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let last_event_id = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                Ok(Self {
                    account_id,
                    projection_id,
                    cursor,
                    last_event_id,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 4usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NonContiguousAdvance {
    pub expected: u64,
    pub proposed: u64,
}
impl golem_wasm::IntoValue for NonContiguousAdvance {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.expected.into_value(), self.proposed.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("expected", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("proposed", < u64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(NonContiguousAdvance).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for NonContiguousAdvance {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let expected = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let proposed = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { expected, proposed })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 2usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CursorConflict {
    pub expected: u64,
    pub actual: u64,
}
impl golem_wasm::IntoValue for CursorConflict {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.expected.into_value(), self.actual.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("expected", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("actual", < u64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(CursorConflict).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for CursorConflict {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let expected = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let actual = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { expected, actual })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 2usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdvanceProjectionCursorInput {
    pub expected_cursor: u64,
    pub next_cursor: u64,
    pub event_id: String,
}
impl golem_wasm::IntoValue for AdvanceProjectionCursorInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.expected_cursor.into_value(), self.next_cursor.into_value(), self
                .event_id.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("expected_cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("next_cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("event_id", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(AdvanceProjectionCursorInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for AdvanceProjectionCursorInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let expected_cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let next_cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let event_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    expected_cursor,
                    next_cursor,
                    event_id,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 3usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ProjectionCursorError {
    InvalidIdentity(InvalidIdentity),
    EmptyEventIdentity,
    NonContiguousAdvance(NonContiguousAdvance),
    CursorConflict(CursorConflict),
    ReplayIdentityConflict(ReplayIdentityConflict),
}
impl golem_wasm::IntoValue for ProjectionCursorError {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::InvalidIdentity(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 0u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::EmptyEventIdentity => {
                golem_wasm::Value::Variant {
                    case_idx: 1u32,
                    case_value: None,
                }
            }
            Self::NonContiguousAdvance(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 2u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::CursorConflict(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 3u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::ReplayIdentityConflict(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 4u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant {
            name: Some(stringify!(ProjectionCursorError).to_string()),
            owner: None,
            cases: vec![
                golem_wasm::analysis::NameOptionTypePair { name : "InvalidIdentity"
                .to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("InvalidIdentity".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "EmptyEventIdentity".to_string(), typ : None, },
                golem_wasm::analysis::NameOptionTypePair { name : "NonContiguousAdvance"
                .to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("NonContiguousAdvance".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "expected".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "proposed".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "CursorConflict".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("CursorConflict".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "expected".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "actual".to_string(), typ
                :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "ReplayIdentityConflict".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("ReplayIdentityConflict".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "cursor".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "existing_event_id"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "proposed_event_id"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }
            ],
        })
    }
}
impl golem_wasm::FromValue for ProjectionCursorError {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Variant { case_idx, case_value } => {
                match case_idx {
                    0u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(InvalidIdentity)
                                )
                            })?;
                        Ok(
                            Self::InvalidIdentity(
                                <InvalidIdentity as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    1u32 => Ok(Self::EmptyEventIdentity),
                    2u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}",
                                    stringify!(NonContiguousAdvance)
                                )
                            })?;
                        Ok(
                            Self::NonContiguousAdvance(
                                <NonContiguousAdvance as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    3u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(CursorConflict)
                                )
                            })?;
                        Ok(
                            Self::CursorConflict(
                                <CursorConflict as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    4u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}",
                                    stringify!(ReplayIdentityConflict)
                                )
                            })?;
                        Ok(
                            Self::ReplayIdentityConflict(
                                <ReplayIdentityConflict as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    _ => Err(format!("Invalid variant case index: {}", case_idx)),
                }
            }
            _ => Err(format!("Expected Variant value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplayIdentityConflict {
    pub cursor: u64,
    pub existing_event_id: String,
    pub proposed_event_id: String,
}
impl golem_wasm::IntoValue for ReplayIdentityConflict {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.cursor.into_value(), self.existing_event_id.into_value(), self
                .proposed_event_id.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("existing_event_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("proposed_event_id", < String as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ReplayIdentityConflict).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ReplayIdentityConflict {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let existing_event_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let proposed_event_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    cursor,
                    existing_event_id,
                    proposed_event_id,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 3usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
