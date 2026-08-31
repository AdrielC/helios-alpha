#![allow(unused)]
use golem_common::base_model::agent::{
    UnstructuredBinaryExtensions, UnstructuredTextExtensions,
};
use golem_wasm::{FromValueAndType, IntoValueAndType};
pub struct OmsAccountAgent {
    constructor_parameters: golem_client::model::UntypedJsonDataValue,
    phantom_id: Option<uuid::Uuid>,
    agent_id: golem_common::model::AgentId,
}
impl std::fmt::Debug for OmsAccountAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OmsAccountAgent))
            .field("constructor_parameters", &self.constructor_parameters)
            .field("phantom_id", &self.phantom_id)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}
impl OmsAccountAgent {
    pub async fn get(
        account_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() })
            ],
        });
        let constructor_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_constructor_parameters
            .into();
        Self::__create(constructor_parameters, None, vec![]).await
    }
    pub async fn get_phantom(
        uuid: uuid::Uuid,
        account_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() })
            ],
        });
        let constructor_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_constructor_parameters
            .into();
        Self::__create(constructor_parameters, Some(uuid), vec![]).await
    }
    pub async fn new_phantom(
        account_id: String,
    ) -> Result<Self, golem_client::bridge::ClientError> {
        let typed_constructor_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : account_id.into_value_and_type() })
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
                    agent_type_name: "OmsAccountAgent".to_string(),
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
    pub async fn acknowledge(
        &self,
        input: VenueAcknowledgementInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__acknowledge(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_acknowledge(
        &self,
        input: VenueAcknowledgementInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__acknowledge(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_acknowledge(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: VenueAcknowledgementInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__acknowledge(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __acknowledge(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: VenueAcknowledgementInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("acknowledge", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn confirm_canceled(
        &self,
        input: OrderActionInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__confirm_canceled(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_confirm_canceled(
        &self,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__confirm_canceled(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_confirm_canceled(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__confirm_canceled(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __confirm_canceled(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: OrderActionInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("confirm_canceled", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn confirm_replaced(
        &self,
        input: ConfirmReplaceInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__confirm_replaced(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_confirm_replaced(
        &self,
        input: ConfirmReplaceInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__confirm_replaced(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_confirm_replaced(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: ConfirmReplaceInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__confirm_replaced(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __confirm_replaced(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: ConfirmReplaceInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("confirm_replaced", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn events_after(
        &self,
        cursor: u64,
        limit: u32,
    ) -> Result<
        Result<EventBatchOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__events_after(
                golem_client::model::AgentInvocationMode::Await,
                None,
                cursor,
                limit,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_events_after(
        &self,
        cursor: u64,
        limit: u32,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__events_after(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                cursor,
                limit,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_events_after(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        cursor: u64,
        limit: u32,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__events_after(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                cursor,
                limit,
            )
            .await?;
        Ok(())
    }
    async fn __events_after(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        cursor: u64,
        limit: u32,
    ) -> Result<
        Option<Result<EventBatchOutput, OmsAgentError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : cursor.into_value_and_type() }),
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : limit.into_value_and_type() })
            ],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self.invoke("events_after", method_parameters, mode, when).await?;
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
                            { name : Some("EventBatchOutput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "next_cursor".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "events_json"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::List(golem_wasm::analysis::TypeList
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        EventBatchOutput,
                                        OmsAgentError,
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
    pub async fn mark_expired(
        &self,
        input: OrderActionInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__mark_expired(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_mark_expired(
        &self,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__mark_expired(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_mark_expired(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__mark_expired(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __mark_expired(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: OrderActionInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("mark_expired", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn mark_unknown(
        &self,
        input: OrderReasonInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__mark_unknown(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_mark_unknown(
        &self,
        input: OrderReasonInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__mark_unknown(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_mark_unknown(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: OrderReasonInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__mark_unknown(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __mark_unknown(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: OrderReasonInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("mark_unknown", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn order(
        &self,
        client_order_id: String,
    ) -> Result<
        Result<Option<OrderView>, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__order(
                golem_client::model::AgentInvocationMode::Await,
                None,
                client_order_id,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_order(
        &self,
        client_order_id: String,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__order(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                client_order_id,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_order(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        client_order_id: String,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__order(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                client_order_id,
            )
            .await?;
        Ok(())
    }
    async fn __order(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        client_order_id: String,
    ) -> Result<
        Option<Result<Option<OrderView>, OmsAgentError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : client_order_id.into_value_and_type() })
            ],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self.invoke("order", method_parameters, mode, when).await?;
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
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderView".to_string()).clone(), owner : None
                            .clone(), fields : vec![golem_wasm::analysis::NameTypePair {
                            name : "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "broker_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }, golem_wasm::analysis::NameTypePair { name : "state"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("OrderStateOutput".to_string()).clone(), owner
                            : None.clone(), cases : vec!["PendingSubmit".to_string(),
                            "Working".to_string(), "PartiallyFilled".to_string(),
                            "PendingCancel".to_string(), "PendingReplace".to_string(),
                            "Filled".to_string(), "Canceled".to_string(), "Rejected"
                            .to_string(), "Expired".to_string(), "Unknown".to_string()],
                            },), }, golem_wasm::analysis::NameTypePair { name : "intent"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderIntentInput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "proposal_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "strategy_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "symbol"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "venue"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "currency"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "side"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("SideInput".to_string()).clone(), owner : None
                            .clone(), cases : vec!["Buy".to_string(), "Sell"
                            .to_string()], },), }, golem_wasm::analysis::NameTypePair {
                            name : "quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "limit_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "execution_mode".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("ExecutionModeInput".to_string()).clone(),
                            owner : None.clone(), cases : vec!["Paper".to_string(),
                            "Live".to_string()], },), },
                            golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "authorized_notional_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "risk_policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "authorized_at_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },), }, golem_wasm::analysis::NameTypePair { name :
                            "time_in_force".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("TimeInForceInput".to_string()).clone(), owner
                            : None.clone(), cases : vec!["Day".to_string(),
                            "GoodTillCanceled".to_string(), "ImmediateOrCancel"
                            .to_string(), "FillOrKill".to_string()], },), },
                            golem_wasm::analysis::NameTypePair { name :
                            "working_quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "working_limit_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "filled_quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "average_fill_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,)),
                            },), }, golem_wasm::analysis::NameTypePair { name :
                            "filled_notional_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "last_update_at_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "uncertainty_reason".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }], },)), },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        Option<OrderView>,
                                        OmsAgentError,
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
    pub async fn orders(
        &self,
        limit: u32,
    ) -> Result<
        Result<Vec<OrderView>, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__orders(golem_client::model::AgentInvocationMode::Await, None, limit)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_orders(
        &self,
        limit: u32,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__orders(golem_client::model::AgentInvocationMode::Schedule, None, limit)
            .await?;
        Ok(())
    }
    pub async fn schedule_orders(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__orders(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                limit,
            )
            .await?;
        Ok(())
    }
    async fn __orders(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        limit: u32,
    ) -> Result<
        Option<Result<Vec<OrderView>, OmsAgentError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : limit.into_value_and_type() })
            ],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self.invoke("orders", method_parameters, mode, when).await?;
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
                            Some(Box::new(golem_wasm::analysis::AnalysedType::List(golem_wasm::analysis::TypeList
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderView".to_string()).clone(), owner : None
                            .clone(), fields : vec![golem_wasm::analysis::NameTypePair {
                            name : "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "broker_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }, golem_wasm::analysis::NameTypePair { name : "state"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("OrderStateOutput".to_string()).clone(), owner
                            : None.clone(), cases : vec!["PendingSubmit".to_string(),
                            "Working".to_string(), "PartiallyFilled".to_string(),
                            "PendingCancel".to_string(), "PendingReplace".to_string(),
                            "Filled".to_string(), "Canceled".to_string(), "Rejected"
                            .to_string(), "Expired".to_string(), "Unknown".to_string()],
                            },), }, golem_wasm::analysis::NameTypePair { name : "intent"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderIntentInput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "proposal_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "strategy_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "symbol"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "venue"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "currency"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "side"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("SideInput".to_string()).clone(), owner : None
                            .clone(), cases : vec!["Buy".to_string(), "Sell"
                            .to_string()], },), }, golem_wasm::analysis::NameTypePair {
                            name : "quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "limit_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "execution_mode".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("ExecutionModeInput".to_string()).clone(),
                            owner : None.clone(), cases : vec!["Paper".to_string(),
                            "Live".to_string()], },), },
                            golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "authorized_notional_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "risk_policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "authorized_at_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }], },), }, golem_wasm::analysis::NameTypePair { name :
                            "time_in_force".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum
                            { name : Some("TimeInForceInput".to_string()).clone(), owner
                            : None.clone(), cases : vec!["Day".to_string(),
                            "GoodTillCanceled".to_string(), "ImmediateOrCancel"
                            .to_string(), "FillOrKill".to_string()], },), },
                            golem_wasm::analysis::NameTypePair { name :
                            "working_quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "working_limit_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "filled_quantity_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "average_fill_price_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,)),
                            },), }, golem_wasm::analysis::NameTypePair { name :
                            "filled_notional_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "last_update_at_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "uncertainty_reason".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Option(golem_wasm::analysis::TypeOption
                            { name : None.clone(), owner : None.clone(), inner :
                            Box::new(golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,)),
                            },), }], },)), },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        Vec<OrderView>,
                                        OmsAgentError,
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
    pub async fn reconcile_unknown(
        &self,
        input: ReconcileUnknownInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__reconcile_unknown(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_reconcile_unknown(
        &self,
        input: ReconcileUnknownInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reconcile_unknown(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_reconcile_unknown(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: ReconcileUnknownInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reconcile_unknown(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __reconcile_unknown(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: ReconcileUnknownInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("reconcile_unknown", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn record_fill(
        &self,
        input: FillInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__record_fill(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_record_fill(
        &self,
        input: FillInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__record_fill(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_record_fill(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: FillInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__record_fill(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __record_fill(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: FillInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("record_fill", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn reject(
        &self,
        input: OrderReasonInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__reject(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_reject(
        &self,
        input: OrderReasonInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reject(golem_client::model::AgentInvocationMode::Schedule, None, input)
            .await?;
        Ok(())
    }
    pub async fn schedule_reject(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: OrderReasonInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reject(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __reject(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: OrderReasonInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("reject", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn reject_pending_action(
        &self,
        input: RejectPendingActionInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__reject_pending_action(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_reject_pending_action(
        &self,
        input: RejectPendingActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reject_pending_action(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_reject_pending_action(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: RejectPendingActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__reject_pending_action(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __reject_pending_action(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: RejectPendingActionInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("reject_pending_action", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn request_cancel(
        &self,
        input: OrderActionInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__request_cancel(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_request_cancel(
        &self,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__request_cancel(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_request_cancel(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: OrderActionInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__request_cancel(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __request_cancel(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: OrderActionInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("request_cancel", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn request_replace(
        &self,
        input: ReplaceOrderInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__request_replace(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_request_replace(
        &self,
        input: ReplaceOrderInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__request_replace(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_request_replace(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: ReplaceOrderInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__request_replace(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __request_replace(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: ReplaceOrderInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self
            .invoke("request_replace", method_parameters, mode, when)
            .await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
    pub async fn submit(
        &self,
        input: SubmitOrderInput,
    ) -> Result<
        Result<CommandReceiptOutput, OmsAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__submit(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_submit(
        &self,
        input: SubmitOrderInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__submit(golem_client::model::AgentInvocationMode::Schedule, None, input)
            .await?;
        Ok(())
    }
    pub async fn schedule_submit(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: SubmitOrderInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__submit(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __submit(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: SubmitOrderInput,
    ) -> Result<
        Option<Result<CommandReceiptOutput, OmsAgentError>>,
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
        let response = self.invoke("submit", method_parameters, mode, when).await?;
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
                            { name : Some("CommandReceiptOutput".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "command_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "client_order_id".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name : "version"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "replayed"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }, golem_wasm::analysis::NameTypePair { name : "event_count"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("OmsAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotInitialized".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotInitialized".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "CommandRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("CommandRejected".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "SerializationFailed".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("SerializationFailed".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "EventBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("EventBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "OrderBatchCapacityExceeded".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("OrderBatchCapacityExceeded".to_string())
                            .clone(), owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "found"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name : "capacity"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
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
                                        CommandReceiptOutput,
                                        OmsAgentError,
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
                    agent_type_name: "OmsAccountAgent".to_string(),
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
pub enum ExecutionModeInput {
    Paper,
    Live,
}
impl golem_wasm::IntoValue for ExecutionModeInput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::Paper => golem_wasm::Value::Enum(0usize as u32),
            Self::Live => golem_wasm::Value::Enum(1usize as u32),
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
            cases: vec!["Paper".to_string(), "Live".to_string()],
            name: Some(stringify!(ExecutionModeInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ExecutionModeInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Enum(idx) => {
                match idx {
                    0u32 => Ok(Self::Paper),
                    1u32 => Ok(Self::Live),
                    _ => Err(format!("Invalid enum index: {}", idx)),
                }
            }
            _ => Err(format!("Expected Enum value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderIntentInput {
    pub client_order_id: String,
    pub proposal_id: String,
    pub strategy_id: String,
    pub symbol: String,
    pub venue: String,
    pub currency: String,
    pub side: SideInput,
    pub quantity_micros: u64,
    pub limit_price_micros: u64,
    pub execution_mode: ExecutionModeInput,
    pub trading_day: i32,
    pub authorized_notional_micros: u64,
    pub risk_policy_version: String,
    pub authorized_at_ns: u64,
}
impl golem_wasm::IntoValue for OrderIntentInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.client_order_id.into_value(), self.proposal_id.into_value(), self
                .strategy_id.into_value(), self.symbol.into_value(), self.venue
                .into_value(), self.currency.into_value(), self.side.into_value(), self
                .quantity_micros.into_value(), self.limit_price_micros.into_value(), self
                .execution_mode.into_value(), self.trading_day.into_value(), self
                .authorized_notional_micros.into_value(), self.risk_policy_version
                .into_value(), self.authorized_at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("proposal_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("strategy_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("symbol", < String as golem_wasm::IntoValue >
                ::get_type()), field("venue", < String as golem_wasm::IntoValue >
                ::get_type()), field("currency", < String as golem_wasm::IntoValue >
                ::get_type()), field("side", < SideInput as golem_wasm::IntoValue >
                ::get_type()), field("quantity_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("limit_price_micros", < u64 as golem_wasm::IntoValue
                > ::get_type()), field("execution_mode", < ExecutionModeInput as
                golem_wasm::IntoValue > ::get_type()), field("trading_day", < i32 as
                golem_wasm::IntoValue > ::get_type()),
                field("authorized_notional_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("risk_policy_version", < String as
                golem_wasm::IntoValue > ::get_type()), field("authorized_at_ns", < u64 as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(OrderIntentInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderIntentInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 14usize => {
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let proposal_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let strategy_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let symbol = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let venue = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let currency = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let side = <SideInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let quantity_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let limit_price_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let execution_mode = <ExecutionModeInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let trading_day = <i32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let authorized_notional_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let risk_policy_version = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let authorized_at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    client_order_id,
                    proposal_id,
                    strategy_id,
                    symbol,
                    venue,
                    currency,
                    side,
                    quantity_micros,
                    limit_price_micros,
                    execution_mode,
                    trading_day,
                    authorized_notional_micros,
                    risk_policy_version,
                    authorized_at_ns,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 14usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderReasonInput {
    pub command_id: String,
    pub client_order_id: String,
    pub reason: String,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for OrderReasonInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .reason.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("reason", < String as golem_wasm::IntoValue >
                ::get_type()), field("at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(OrderReasonInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderReasonInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let reason = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    reason,
                    at_ns,
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
pub struct FillInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub execution_id: String,
    pub venue_occurred_at: Option<String>,
    pub quantity_micros: u64,
    pub price_micros: u64,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for FillInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .broker_order_id.into_value(), self.execution_id.into_value(), self
                .venue_occurred_at.into_value(), self.quantity_micros.into_value(), self
                .price_micros.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("broker_order_id", < Option < String > as
                golem_wasm::IntoValue > ::get_type()), field("execution_id", < String as
                golem_wasm::IntoValue > ::get_type()), field("venue_occurred_at", <
                Option < String > as golem_wasm::IntoValue > ::get_type()),
                field("quantity_micros", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("price_micros", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("at_ns", < u64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(FillInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for FillInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 8usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let broker_order_id = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let execution_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let venue_occurred_at = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let quantity_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let price_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    execution_id,
                    venue_occurred_at,
                    quantity_micros,
                    price_micros,
                    at_ns,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 8usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventBatchOutput {
    pub next_cursor: u64,
    pub events_json: Vec<String>,
}
impl golem_wasm::IntoValue for EventBatchOutput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.next_cursor.into_value(), self.events_json.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("next_cursor", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("events_json", < Vec < String > as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(EventBatchOutput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for EventBatchOutput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let next_cursor = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let events_json = <Vec<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                Ok(Self { next_cursor, events_json })
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
pub struct SerializationFailed {
    pub detail: String,
}
impl golem_wasm::IntoValue for SerializationFailed {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(SerializationFailed).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for SerializationFailed {
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
pub struct OrderView {
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub state: OrderStateOutput,
    pub intent: OrderIntentInput,
    pub time_in_force: TimeInForceInput,
    pub working_quantity_micros: u64,
    pub working_limit_price_micros: u64,
    pub filled_quantity_micros: u64,
    pub average_fill_price_micros: Option<u64>,
    pub filled_notional_micros: u64,
    pub version: u64,
    pub last_update_at_ns: u64,
    pub uncertainty_reason: Option<String>,
}
impl golem_wasm::IntoValue for OrderView {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.client_order_id.into_value(), self.broker_order_id.into_value(),
                self.state.into_value(), self.intent.into_value(), self.time_in_force
                .into_value(), self.working_quantity_micros.into_value(), self
                .working_limit_price_micros.into_value(), self.filled_quantity_micros
                .into_value(), self.average_fill_price_micros.into_value(), self
                .filled_notional_micros.into_value(), self.version.into_value(), self
                .last_update_at_ns.into_value(), self.uncertainty_reason.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("broker_order_id", < Option < String > as
                golem_wasm::IntoValue > ::get_type()), field("state", < OrderStateOutput
                as golem_wasm::IntoValue > ::get_type()), field("intent", <
                OrderIntentInput as golem_wasm::IntoValue > ::get_type()),
                field("time_in_force", < TimeInForceInput as golem_wasm::IntoValue >
                ::get_type()), field("working_quantity_micros", < u64 as
                golem_wasm::IntoValue > ::get_type()),
                field("working_limit_price_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("filled_quantity_micros", < u64 as
                golem_wasm::IntoValue > ::get_type()), field("average_fill_price_micros",
                < Option < u64 > as golem_wasm::IntoValue > ::get_type()),
                field("filled_notional_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("version", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("last_update_at_ns", < u64 as golem_wasm::IntoValue
                > ::get_type()), field("uncertainty_reason", < Option < String > as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(OrderView).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderView {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 13usize => {
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let broker_order_id = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let state = <OrderStateOutput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let intent = <OrderIntentInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let time_in_force = <TimeInForceInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let working_quantity_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let working_limit_price_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let filled_quantity_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let average_fill_price_micros = <Option<
                    u64,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let filled_notional_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let version = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let last_update_at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let uncertainty_reason = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                Ok(Self {
                    client_order_id,
                    broker_order_id,
                    state,
                    intent,
                    time_in_force,
                    working_quantity_micros,
                    working_limit_price_micros,
                    filled_quantity_micros,
                    average_fill_price_micros,
                    filled_notional_micros,
                    version,
                    last_update_at_ns,
                    uncertainty_reason,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 13usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TimeInForceInput {
    Day,
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}
impl golem_wasm::IntoValue for TimeInForceInput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::Day => golem_wasm::Value::Enum(0usize as u32),
            Self::GoodTillCanceled => golem_wasm::Value::Enum(1usize as u32),
            Self::ImmediateOrCancel => golem_wasm::Value::Enum(2usize as u32),
            Self::FillOrKill => golem_wasm::Value::Enum(3usize as u32),
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
            cases: vec![
                "Day".to_string(), "GoodTillCanceled".to_string(), "ImmediateOrCancel"
                .to_string(), "FillOrKill".to_string()
            ],
            name: Some(stringify!(TimeInForceInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for TimeInForceInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Enum(idx) => {
                match idx {
                    0u32 => Ok(Self::Day),
                    1u32 => Ok(Self::GoodTillCanceled),
                    2u32 => Ok(Self::ImmediateOrCancel),
                    3u32 => Ok(Self::FillOrKill),
                    _ => Err(format!("Invalid enum index: {}", idx)),
                }
            }
            _ => Err(format!("Expected Enum value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventBatchCapacityExceeded {
    pub found: u32,
    pub capacity: u32,
}
impl golem_wasm::IntoValue for EventBatchCapacityExceeded {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.found.into_value(), self.capacity.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("found", < u32 as golem_wasm::IntoValue > ::get_type()),
                field("capacity", < u32 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(EventBatchCapacityExceeded).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for EventBatchCapacityExceeded {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let found = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let capacity = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { found, capacity })
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
pub struct OrderBatchCapacityExceeded {
    pub found: u32,
    pub capacity: u32,
}
impl golem_wasm::IntoValue for OrderBatchCapacityExceeded {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.found.into_value(), self.capacity.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("found", < u32 as golem_wasm::IntoValue > ::get_type()),
                field("capacity", < u32 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(OrderBatchCapacityExceeded).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderBatchCapacityExceeded {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let found = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let capacity = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { found, capacity })
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
pub enum OmsAgentError {
    NotInitialized(NotInitialized),
    CommandRejected(CommandRejected),
    SerializationFailed(SerializationFailed),
    EventBatchCapacityExceeded(EventBatchCapacityExceeded),
    OrderBatchCapacityExceeded(OrderBatchCapacityExceeded),
}
impl golem_wasm::IntoValue for OmsAgentError {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::NotInitialized(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 0u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::CommandRejected(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 1u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::SerializationFailed(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 2u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::EventBatchCapacityExceeded(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 3u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::OrderBatchCapacityExceeded(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 4u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant {
            name: Some(stringify!(OmsAgentError).to_string()),
            owner: None,
            cases: vec![
                golem_wasm::analysis::NameOptionTypePair { name : "NotInitialized"
                .to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("NotInitialized".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "CommandRejected".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("CommandRejected".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "SerializationFailed".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("SerializationFailed".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "EventBatchCapacityExceeded".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("EventBatchCapacityExceeded".to_string()).clone(), owner :
                None.clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "found".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                }, golem_wasm::analysis::NameTypePair { name : "capacity".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "OrderBatchCapacityExceeded".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("OrderBatchCapacityExceeded".to_string()).clone(), owner :
                None.clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "found".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                }, golem_wasm::analysis::NameTypePair { name : "capacity".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                }], },)), }
            ],
        })
    }
}
impl golem_wasm::FromValue for OmsAgentError {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Variant { case_idx, case_value } => {
                match case_idx {
                    0u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(NotInitialized)
                                )
                            })?;
                        Ok(
                            Self::NotInitialized(
                                <NotInitialized as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    1u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(CommandRejected)
                                )
                            })?;
                        Ok(
                            Self::CommandRejected(
                                <CommandRejected as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    2u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}",
                                    stringify!(SerializationFailed)
                                )
                            })?;
                        Ok(
                            Self::SerializationFailed(
                                <SerializationFailed as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    3u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}",
                                    stringify!(EventBatchCapacityExceeded)
                                )
                            })?;
                        Ok(
                            Self::EventBatchCapacityExceeded(
                                <EventBatchCapacityExceeded as golem_wasm::FromValue>::from_value(
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
                                    stringify!(OrderBatchCapacityExceeded)
                                )
                            })?;
                        Ok(
                            Self::OrderBatchCapacityExceeded(
                                <OrderBatchCapacityExceeded as golem_wasm::FromValue>::from_value(
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
pub enum ReconciledStateInput {
    Working,
    Canceled,
    Rejected,
    Expired,
}
impl golem_wasm::IntoValue for ReconciledStateInput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::Working => golem_wasm::Value::Enum(0usize as u32),
            Self::Canceled => golem_wasm::Value::Enum(1usize as u32),
            Self::Rejected => golem_wasm::Value::Enum(2usize as u32),
            Self::Expired => golem_wasm::Value::Enum(3usize as u32),
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
            cases: vec![
                "Working".to_string(), "Canceled".to_string(), "Rejected".to_string(),
                "Expired".to_string()
            ],
            name: Some(stringify!(ReconciledStateInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ReconciledStateInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Enum(idx) => {
                match idx {
                    0u32 => Ok(Self::Working),
                    1u32 => Ok(Self::Canceled),
                    2u32 => Ok(Self::Rejected),
                    3u32 => Ok(Self::Expired),
                    _ => Err(format!("Invalid enum index: {}", idx)),
                }
            }
            _ => Err(format!("Expected Enum value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReconcileUnknownInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub state: ReconciledStateInput,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for ReconcileUnknownInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .broker_order_id.into_value(), self.state.into_value(), self.at_ns
                .into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("broker_order_id", < Option < String > as
                golem_wasm::IntoValue > ::get_type()), field("state", <
                ReconciledStateInput as golem_wasm::IntoValue > ::get_type()),
                field("at_ns", < u64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ReconcileUnknownInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ReconcileUnknownInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 5usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let broker_order_id = <Option<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let state = <ReconciledStateInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    state,
                    at_ns,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 5usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VenueAcknowledgementInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: String,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for VenueAcknowledgementInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .broker_order_id.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("broker_order_id", < String as golem_wasm::IntoValue
                > ::get_type()), field("at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(VenueAcknowledgementInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for VenueAcknowledgementInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let broker_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    at_ns,
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
pub struct RejectPendingActionInput {
    pub command_id: String,
    pub client_order_id: String,
    pub reason: String,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for RejectPendingActionInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .reason.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("reason", < String as golem_wasm::IntoValue >
                ::get_type()), field("at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(RejectPendingActionInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for RejectPendingActionInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let reason = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    reason,
                    at_ns,
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
pub struct CommandReceiptOutput {
    pub command_id: String,
    pub client_order_id: String,
    pub version: u64,
    pub replayed: bool,
    pub event_count: u32,
}
impl golem_wasm::IntoValue for CommandReceiptOutput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .version.into_value(), self.replayed.into_value(), self.event_count
                .into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("version", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("replayed", < bool as golem_wasm::IntoValue >
                ::get_type()), field("event_count", < u32 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(CommandReceiptOutput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for CommandReceiptOutput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 5usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let version = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let replayed = <bool as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let event_count = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    version,
                    replayed,
                    event_count,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 5usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderActionInput {
    pub command_id: String,
    pub client_order_id: String,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for OrderActionInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(OrderActionInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderActionInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    at_ns,
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
pub enum OrderStateOutput {
    PendingSubmit,
    Working,
    PartiallyFilled,
    PendingCancel,
    PendingReplace,
    Filled,
    Canceled,
    Rejected,
    Expired,
    Unknown,
}
impl golem_wasm::IntoValue for OrderStateOutput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::PendingSubmit => golem_wasm::Value::Enum(0usize as u32),
            Self::Working => golem_wasm::Value::Enum(1usize as u32),
            Self::PartiallyFilled => golem_wasm::Value::Enum(2usize as u32),
            Self::PendingCancel => golem_wasm::Value::Enum(3usize as u32),
            Self::PendingReplace => golem_wasm::Value::Enum(4usize as u32),
            Self::Filled => golem_wasm::Value::Enum(5usize as u32),
            Self::Canceled => golem_wasm::Value::Enum(6usize as u32),
            Self::Rejected => golem_wasm::Value::Enum(7usize as u32),
            Self::Expired => golem_wasm::Value::Enum(8usize as u32),
            Self::Unknown => golem_wasm::Value::Enum(9usize as u32),
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
            cases: vec![
                "PendingSubmit".to_string(), "Working".to_string(), "PartiallyFilled"
                .to_string(), "PendingCancel".to_string(), "PendingReplace".to_string(),
                "Filled".to_string(), "Canceled".to_string(), "Rejected".to_string(),
                "Expired".to_string(), "Unknown".to_string()
            ],
            name: Some(stringify!(OrderStateOutput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for OrderStateOutput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Enum(idx) => {
                match idx {
                    0u32 => Ok(Self::PendingSubmit),
                    1u32 => Ok(Self::Working),
                    2u32 => Ok(Self::PartiallyFilled),
                    3u32 => Ok(Self::PendingCancel),
                    4u32 => Ok(Self::PendingReplace),
                    5u32 => Ok(Self::Filled),
                    6u32 => Ok(Self::Canceled),
                    7u32 => Ok(Self::Rejected),
                    8u32 => Ok(Self::Expired),
                    9u32 => Ok(Self::Unknown),
                    _ => Err(format!("Invalid enum index: {}", idx)),
                }
            }
            _ => Err(format!("Expected Enum value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplaceOrderInput {
    pub command_id: String,
    pub client_order_id: String,
    pub new_quantity_micros: u64,
    pub new_limit_price_micros: u64,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for ReplaceOrderInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .new_quantity_micros.into_value(), self.new_limit_price_micros
                .into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("new_quantity_micros", < u64 as
                golem_wasm::IntoValue > ::get_type()), field("new_limit_price_micros", <
                u64 as golem_wasm::IntoValue > ::get_type()), field("at_ns", < u64 as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ReplaceOrderInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ReplaceOrderInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 5usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let new_quantity_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let new_limit_price_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    new_quantity_micros,
                    new_limit_price_micros,
                    at_ns,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 5usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotInitialized {
    pub detail: String,
}
impl golem_wasm::IntoValue for NotInitialized {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(NotInitialized).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for NotInitialized {
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
pub enum SideInput {
    Buy,
    Sell,
}
impl golem_wasm::IntoValue for SideInput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::Buy => golem_wasm::Value::Enum(0usize as u32),
            Self::Sell => golem_wasm::Value::Enum(1usize as u32),
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
            cases: vec!["Buy".to_string(), "Sell".to_string()],
            name: Some(stringify!(SideInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for SideInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Enum(idx) => {
                match idx {
                    0u32 => Ok(Self::Buy),
                    1u32 => Ok(Self::Sell),
                    _ => Err(format!("Invalid enum index: {}", idx)),
                }
            }
            _ => Err(format!("Expected Enum value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfirmReplaceInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: String,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for ConfirmReplaceInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.client_order_id.into_value(), self
                .broker_order_id.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("client_order_id", < String as golem_wasm::IntoValue >
                ::get_type()), field("broker_order_id", < String as golem_wasm::IntoValue
                > ::get_type()), field("at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(ConfirmReplaceInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ConfirmReplaceInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let client_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let broker_order_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    at_ns,
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
pub struct SubmitOrderInput {
    pub command_id: String,
    pub intent: OrderIntentInput,
    pub time_in_force: TimeInForceInput,
    pub at_ns: u64,
}
impl golem_wasm::IntoValue for SubmitOrderInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.command_id.into_value(), self.intent.into_value(), self
                .time_in_force.into_value(), self.at_ns.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("command_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("intent", < OrderIntentInput as golem_wasm::IntoValue >
                ::get_type()), field("time_in_force", < TimeInForceInput as
                golem_wasm::IntoValue > ::get_type()), field("at_ns", < u64 as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(SubmitOrderInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for SubmitOrderInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 4usize => {
                let command_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let intent = <OrderIntentInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let time_in_force = <TimeInForceInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    command_id,
                    intent,
                    time_in_force,
                    at_ns,
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
pub struct CommandRejected {
    pub detail: String,
}
impl golem_wasm::IntoValue for CommandRejected {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(CommandRejected).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for CommandRejected {
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
