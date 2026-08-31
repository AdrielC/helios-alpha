#![allow(unused)]
use golem_common::base_model::agent::{
    UnstructuredBinaryExtensions, UnstructuredTextExtensions,
};
use golem_wasm::{FromValueAndType, IntoValueAndType};
pub struct RiskAccountAgent {
    constructor_parameters: golem_client::model::UntypedJsonDataValue,
    phantom_id: Option<uuid::Uuid>,
    agent_id: golem_common::model::AgentId,
}
impl std::fmt::Debug for RiskAccountAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RiskAccountAgent))
            .field("constructor_parameters", &self.constructor_parameters)
            .field("phantom_id", &self.phantom_id)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}
impl RiskAccountAgent {
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
                    agent_type_name: "RiskAccountAgent".to_string(),
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
    pub async fn authorize(
        &self,
        input: AuthorizeRiskInput,
    ) -> Result<
        Result<RiskDecisionOutput, RiskAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__authorize(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_authorize(
        &self,
        input: AuthorizeRiskInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__authorize(golem_client::model::AgentInvocationMode::Schedule, None, input)
            .await?;
        Ok(())
    }
    pub async fn schedule_authorize(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: AuthorizeRiskInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__authorize(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __authorize(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: AuthorizeRiskInput,
    ) -> Result<
        Option<Result<RiskDecisionOutput, RiskAgentError>>,
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
        let response = self.invoke("authorize", method_parameters, mode, when).await?;
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
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskDecisionOutput".to_string()).clone(),
                            owner : None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "Approved".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("Approved".to_string()).clone(), owner : None
                            .clone(), fields : vec![golem_wasm::analysis::NameTypePair {
                            name : "intent".to_string(), typ :
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
                            }], },), }], },)), },
                            golem_wasm::analysis::NameOptionTypePair { name : "Rejected"
                            .to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("Rejected".to_string()).clone(), owner : None
                            .clone(), fields : vec![golem_wasm::analysis::NameTypePair {
                            name : "reason".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotConfigured".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotConfigured".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidConfiguration".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidConfiguration".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ConfigurationConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ConfigurationConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "existing_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidPortfolio".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidPortfolio".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "AuthorityRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("AuthorityRejected".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
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
                                        RiskDecisionOutput,
                                        RiskAgentError,
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
    pub async fn configure(
        &self,
        input: ConfigureRiskInput,
    ) -> Result<
        Result<RiskStatusOutput, RiskAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__configure(golem_client::model::AgentInvocationMode::Await, None, input)
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_configure(
        &self,
        input: ConfigureRiskInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__configure(golem_client::model::AgentInvocationMode::Schedule, None, input)
            .await?;
        Ok(())
    }
    pub async fn schedule_configure(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: ConfigureRiskInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__configure(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __configure(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: ConfigureRiskInput,
    ) -> Result<
        Option<Result<RiskStatusOutput, RiskAgentError>>,
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
        let response = self.invoke("configure", method_parameters, mode, when).await?;
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
                            { name : Some("RiskStatusOutput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "account_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "configuration_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "portfolio_as_of_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "gross_exposure_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_gross_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_order_count".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "outstanding_reservations".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "kill_switch_active".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotConfigured".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotConfigured".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidConfiguration".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidConfiguration".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ConfigurationConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ConfigurationConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "existing_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidPortfolio".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidPortfolio".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "AuthorityRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("AuthorityRejected".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
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
                                        RiskStatusOutput,
                                        RiskAgentError,
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
    pub async fn refresh_portfolio(
        &self,
        input: RefreshPortfolioInput,
    ) -> Result<
        Result<RiskStatusOutput, RiskAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__refresh_portfolio(
                golem_client::model::AgentInvocationMode::Await,
                None,
                input,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_refresh_portfolio(
        &self,
        input: RefreshPortfolioInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__refresh_portfolio(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                input,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_refresh_portfolio(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        input: RefreshPortfolioInput,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__refresh_portfolio(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                input,
            )
            .await?;
        Ok(())
    }
    async fn __refresh_portfolio(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        input: RefreshPortfolioInput,
    ) -> Result<
        Option<Result<RiskStatusOutput, RiskAgentError>>,
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
            .invoke("refresh_portfolio", method_parameters, mode, when)
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
                            { name : Some("RiskStatusOutput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "account_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "configuration_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "portfolio_as_of_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "gross_exposure_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_gross_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_order_count".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "outstanding_reservations".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "kill_switch_active".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotConfigured".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotConfigured".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidConfiguration".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidConfiguration".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ConfigurationConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ConfigurationConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "existing_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidPortfolio".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidPortfolio".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "AuthorityRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("AuthorityRejected".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
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
                                        RiskStatusOutput,
                                        RiskAgentError,
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
    pub async fn set_kill_switch(
        &self,
        active: bool,
    ) -> Result<
        Result<RiskStatusOutput, RiskAgentError>,
        golem_client::bridge::ClientError,
    > {
        let result = self
            .__set_kill_switch(
                golem_client::model::AgentInvocationMode::Await,
                None,
                active,
            )
            .await?;
        let result = result.unwrap();
        Ok(result)
    }
    pub async fn trigger_set_kill_switch(
        &self,
        active: bool,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__set_kill_switch(
                golem_client::model::AgentInvocationMode::Schedule,
                None,
                active,
            )
            .await?;
        Ok(())
    }
    pub async fn schedule_set_kill_switch(
        &self,
        when: chrono::DateTime<chrono::Utc>,
        active: bool,
    ) -> Result<(), golem_client::bridge::ClientError> {
        let _ = self
            .__set_kill_switch(
                golem_client::model::AgentInvocationMode::Schedule,
                Some(when),
                active,
            )
            .await?;
        Ok(())
    }
    async fn __set_kill_switch(
        &self,
        mode: golem_client::model::AgentInvocationMode,
        when: Option<chrono::DateTime<chrono::Utc>>,
        active: bool,
    ) -> Result<
        Option<Result<RiskStatusOutput, RiskAgentError>>,
        golem_client::bridge::ClientError,
    > {
        let typed_method_parameters = golem_common::model::agent::DataValue::Tuple(golem_common::model::agent::ElementValues {
            elements: vec![
                golem_common::model::agent::ElementValue::ComponentModel(golem_common::model::agent::ComponentModelElementValue
                { value : active.into_value_and_type() })
            ],
        });
        let method_parameters: golem_common::model::agent::UntypedJsonDataValue = typed_method_parameters
            .into();
        let response = self
            .invoke("set_kill_switch", method_parameters, mode, when)
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
                            { name : Some("RiskStatusOutput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "account_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "configuration_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "portfolio_as_of_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "gross_exposure_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_gross_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_order_count".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "outstanding_reservations".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "kill_switch_active".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotConfigured".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotConfigured".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidConfiguration".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidConfiguration".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ConfigurationConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ConfigurationConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "existing_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidPortfolio".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidPortfolio".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "AuthorityRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("AuthorityRejected".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
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
                                        RiskStatusOutput,
                                        RiskAgentError,
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
        Result<RiskStatusOutput, RiskAgentError>,
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
        Option<Result<RiskStatusOutput, RiskAgentError>>,
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
                            { name : Some("RiskStatusOutput".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "account_id"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "configuration_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "policy_version".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "portfolio_as_of_ns".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name : "trading_day"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "gross_exposure_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_gross_micros".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "reserved_order_count".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U32(golem_wasm::analysis::TypeU32,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "outstanding_reservations".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "kill_switch_active".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Bool(golem_wasm::analysis::TypeBool,),
                            }], },))), err :
                            Some(Box::new(golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant
                            { name : Some("RiskAgentError".to_string()).clone(), owner :
                            None.clone(), cases :
                            vec![golem_wasm::analysis::NameOptionTypePair { name :
                            "NotConfigured".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("NotConfigured".to_string()).clone(), owner :
                            None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidConfiguration".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidConfiguration".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "ConfigurationConflict".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("ConfigurationConflict".to_string()).clone(),
                            owner : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name :
                            "existing_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }, golem_wasm::analysis::NameTypePair { name :
                            "proposed_fingerprint".to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "InvalidPortfolio".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("InvalidPortfolio".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
                            golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                            }], },)), }, golem_wasm::analysis::NameOptionTypePair { name
                            : "AuthorityRejected".to_string(), typ :
                            Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                            { name : Some("AuthorityRejected".to_string()).clone(), owner
                            : None.clone(), fields :
                            vec![golem_wasm::analysis::NameTypePair { name : "detail"
                            .to_string(), typ :
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
                                        RiskStatusOutput,
                                        RiskAgentError,
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
                    agent_type_name: "RiskAccountAgent".to_string(),
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
pub struct NamedExposureInput {
    pub name: String,
    pub amount_micros: u64,
}
impl golem_wasm::IntoValue for NamedExposureInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.name.into_value(), self.amount_micros.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("name", < String as golem_wasm::IntoValue > ::get_type()),
                field("amount_micros", < u64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(NamedExposureInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for NamedExposureInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let name = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let amount_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { name, amount_micros })
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
pub struct SymbolPositionInput {
    pub symbol: String,
    pub quantity_micros: i64,
}
impl golem_wasm::IntoValue for SymbolPositionInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.symbol.into_value(), self.quantity_micros.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("symbol", < String as golem_wasm::IntoValue > ::get_type()),
                field("quantity_micros", < i64 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(SymbolPositionInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for SymbolPositionInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let symbol = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let quantity_micros = <i64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { symbol, quantity_micros })
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
pub struct RiskContextInput {
    pub now_ns: u64,
    pub market_data_at_ns: u64,
    pub venue_time_utc_sec: i64,
}
impl golem_wasm::IntoValue for RiskContextInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.now_ns.into_value(), self.market_data_at_ns.into_value(), self
                .venue_time_utc_sec.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("now_ns", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("market_data_at_ns", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("venue_time_utc_sec", < i64 as golem_wasm::IntoValue
                > ::get_type())
            ],
            name: Some(stringify!(RiskContextInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for RiskContextInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let now_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let market_data_at_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let venue_time_utc_sec = <i64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    now_ns,
                    market_data_at_ns,
                    venue_time_utc_sec,
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
pub enum RiskDecisionOutput {
    Approved(Approved),
    Rejected(Rejected),
}
impl golem_wasm::IntoValue for RiskDecisionOutput {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::Approved(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 0u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::Rejected(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 1u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant {
            name: Some(stringify!(RiskDecisionOutput).to_string()),
            owner: None,
            cases: vec![
                golem_wasm::analysis::NameOptionTypePair { name : "Approved".to_string(),
                typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("Approved".to_string()).clone(), owner : None.clone(),
                fields : vec![golem_wasm::analysis::NameTypePair { name : "intent"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("OrderIntentInput".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "client_order_id".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "proposal_id".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "strategy_id".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "symbol".to_string(), typ
                :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "venue".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "currency".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "side".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
                name : Some("SideInput".to_string()).clone(), owner : None.clone(), cases
                : vec!["Buy".to_string(), "Sell".to_string()], },), },
                golem_wasm::analysis::NameTypePair { name : "quantity_micros"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "limit_price_micros"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "execution_mode"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Enum(golem_wasm::analysis::TypeEnum {
                name : Some("ExecutionModeInput".to_string()).clone(), owner : None
                .clone(), cases : vec!["Paper".to_string(), "Live".to_string()], },), },
                golem_wasm::analysis::NameTypePair { name : "trading_day".to_string(),
                typ :
                golem_wasm::analysis::AnalysedType::S32(golem_wasm::analysis::TypeS32,),
                }, golem_wasm::analysis::NameTypePair { name :
                "authorized_notional_micros".to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }, golem_wasm::analysis::NameTypePair { name : "risk_policy_version"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "authorized_at_ns"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::U64(golem_wasm::analysis::TypeU64,),
                }], },), }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "Rejected".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("Rejected".to_string()).clone(), owner : None.clone(),
                fields : vec![golem_wasm::analysis::NameTypePair { name : "reason"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }
            ],
        })
    }
}
impl golem_wasm::FromValue for RiskDecisionOutput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Variant { case_idx, case_value } => {
                match case_idx {
                    0u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!("Expected case_value for {}", stringify!(Approved))
                            })?;
                        Ok(
                            Self::Approved(
                                <Approved as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    1u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!("Expected case_value for {}", stringify!(Rejected))
                            })?;
                        Ok(
                            Self::Rejected(
                                <Rejected as golem_wasm::FromValue>::from_value(
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
pub struct NotConfigured {
    pub detail: String,
}
impl golem_wasm::IntoValue for NotConfigured {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(NotConfigured).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for NotConfigured {
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
pub struct InvalidConfiguration {
    pub detail: String,
}
impl golem_wasm::IntoValue for InvalidConfiguration {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(InvalidConfiguration).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for InvalidConfiguration {
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
pub struct AuthorityRejected {
    pub detail: String,
}
impl golem_wasm::IntoValue for AuthorityRejected {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(AuthorityRejected).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for AuthorityRejected {
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
pub struct ConfigureRiskInput {
    pub risk_policy_json: String,
    pub venue_schedule_json: String,
    pub initial_portfolio: PortfolioRiskInput,
}
impl golem_wasm::IntoValue for ConfigureRiskInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.risk_policy_json.into_value(), self.venue_schedule_json
                .into_value(), self.initial_portfolio.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("risk_policy_json", < String as golem_wasm::IntoValue >
                ::get_type()), field("venue_schedule_json", < String as
                golem_wasm::IntoValue > ::get_type()), field("initial_portfolio", <
                PortfolioRiskInput as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ConfigureRiskInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ConfigureRiskInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let risk_policy_json = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let venue_schedule_json = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let initial_portfolio = <PortfolioRiskInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    risk_policy_json,
                    venue_schedule_json,
                    initial_portfolio,
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
pub struct RefreshPortfolioInput {
    pub portfolio: PortfolioRiskInput,
    pub covered_client_order_ids: Vec<String>,
}
impl golem_wasm::IntoValue for RefreshPortfolioInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![self.portfolio.into_value(), self.covered_client_order_ids.into_value()],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("portfolio", < PortfolioRiskInput as golem_wasm::IntoValue >
                ::get_type()), field("covered_client_order_ids", < Vec < String > as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(RefreshPortfolioInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for RefreshPortfolioInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let portfolio = <PortfolioRiskInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let covered_client_order_ids = <Vec<
                    String,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                Ok(Self {
                    portfolio,
                    covered_client_order_ids,
                })
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
pub struct InvalidPortfolio {
    pub detail: String,
}
impl golem_wasm::IntoValue for InvalidPortfolio {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.detail.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("detail", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(InvalidPortfolio).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for InvalidPortfolio {
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
pub struct Rejected {
    pub reason: String,
}
impl golem_wasm::IntoValue for Rejected {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.reason.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("reason", < String as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(Rejected).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for Rejected {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 1usize => {
                let reason = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { reason })
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
pub struct Approved {
    pub intent: OrderIntentInput,
}
impl golem_wasm::IntoValue for Approved {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(vec![self.intent.into_value()])
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("intent", < OrderIntentInput as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(Approved).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for Approved {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 1usize => {
                let intent = <OrderIntentInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self { intent })
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
pub struct ConfigurationConflict {
    pub existing_fingerprint: String,
    pub proposed_fingerprint: String,
}
impl golem_wasm::IntoValue for ConfigurationConflict {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.existing_fingerprint.into_value(), self.proposed_fingerprint
                .into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("existing_fingerprint", < String as golem_wasm::IntoValue >
                ::get_type()), field("proposed_fingerprint", < String as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(ConfigurationConflict).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for ConfigurationConflict {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 2usize => {
                let existing_fingerprint = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let proposed_fingerprint = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    existing_fingerprint,
                    proposed_fingerprint,
                })
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
pub struct PortfolioRiskInput {
    pub as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure_micros: u64,
    pub strategy_exposure: Vec<NamedExposureInput>,
    pub symbol_positions: Vec<SymbolPositionInput>,
    pub daily_order_count: u32,
}
impl golem_wasm::IntoValue for PortfolioRiskInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.as_of_ns.into_value(), self.trading_day.into_value(), self
                .gross_exposure_micros.into_value(), self.strategy_exposure.into_value(),
                self.symbol_positions.into_value(), self.daily_order_count.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("as_of_ns", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("trading_day", < i32 as golem_wasm::IntoValue > ::get_type()),
                field("gross_exposure_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("strategy_exposure", < Vec < NamedExposureInput > as
                golem_wasm::IntoValue > ::get_type()), field("symbol_positions", < Vec <
                SymbolPositionInput > as golem_wasm::IntoValue > ::get_type()),
                field("daily_order_count", < u32 as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(PortfolioRiskInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for PortfolioRiskInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 6usize => {
                let as_of_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let trading_day = <i32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let gross_exposure_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let strategy_exposure = <Vec<
                    NamedExposureInput,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let symbol_positions = <Vec<
                    SymbolPositionInput,
                > as golem_wasm::FromValue>::from_value(fields.remove(0))?;
                let daily_order_count = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    as_of_ns,
                    trading_day,
                    gross_exposure_micros,
                    strategy_exposure,
                    symbol_positions,
                    daily_order_count,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 6usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RiskStatusOutput {
    pub account_id: String,
    pub configuration_fingerprint: String,
    pub policy_version: String,
    pub portfolio_as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure_micros: u64,
    pub reserved_gross_micros: u64,
    pub reserved_order_count: u32,
    pub outstanding_reservations: u64,
    pub kill_switch_active: bool,
}
impl golem_wasm::IntoValue for RiskStatusOutput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.account_id.into_value(), self.configuration_fingerprint
                .into_value(), self.policy_version.into_value(), self.portfolio_as_of_ns
                .into_value(), self.trading_day.into_value(), self.gross_exposure_micros
                .into_value(), self.reserved_gross_micros.into_value(), self
                .reserved_order_count.into_value(), self.outstanding_reservations
                .into_value(), self.kill_switch_active.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("account_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("configuration_fingerprint", < String as golem_wasm::IntoValue >
                ::get_type()), field("policy_version", < String as golem_wasm::IntoValue
                > ::get_type()), field("portfolio_as_of_ns", < u64 as
                golem_wasm::IntoValue > ::get_type()), field("trading_day", < i32 as
                golem_wasm::IntoValue > ::get_type()), field("gross_exposure_micros", <
                u64 as golem_wasm::IntoValue > ::get_type()),
                field("reserved_gross_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("reserved_order_count", < u32 as
                golem_wasm::IntoValue > ::get_type()), field("outstanding_reservations",
                < u64 as golem_wasm::IntoValue > ::get_type()),
                field("kill_switch_active", < bool as golem_wasm::IntoValue >
                ::get_type())
            ],
            name: Some(stringify!(RiskStatusOutput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for RiskStatusOutput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 10usize => {
                let account_id = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let configuration_fingerprint = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let policy_version = <String as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let portfolio_as_of_ns = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let trading_day = <i32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let gross_exposure_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let reserved_gross_micros = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let reserved_order_count = <u32 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let outstanding_reservations = <u64 as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let kill_switch_active = <bool as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    account_id,
                    configuration_fingerprint,
                    policy_version,
                    portfolio_as_of_ns,
                    trading_day,
                    gross_exposure_micros,
                    reserved_gross_micros,
                    reserved_order_count,
                    outstanding_reservations,
                    kill_switch_active,
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 10usize, fields.len()
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
pub struct AuthorizeRiskInput {
    pub proposal: RiskProposalInput,
    pub context: RiskContextInput,
    pub portfolio: PortfolioRiskInput,
}
impl golem_wasm::IntoValue for AuthorizeRiskInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.proposal.into_value(), self.context.into_value(), self.portfolio
                .into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("proposal", < RiskProposalInput as golem_wasm::IntoValue >
                ::get_type()), field("context", < RiskContextInput as
                golem_wasm::IntoValue > ::get_type()), field("portfolio", <
                PortfolioRiskInput as golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(AuthorizeRiskInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for AuthorizeRiskInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 3usize => {
                let proposal = <RiskProposalInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let context = <RiskContextInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                let portfolio = <PortfolioRiskInput as golem_wasm::FromValue>::from_value(
                    fields.remove(0),
                )?;
                Ok(Self {
                    proposal,
                    context,
                    portfolio,
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
pub struct RiskProposalInput {
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
}
impl golem_wasm::IntoValue for RiskProposalInput {
    fn into_value(self) -> golem_wasm::Value {
        golem_wasm::Value::Record(
            vec![
                self.proposal_id.into_value(), self.strategy_id.into_value(), self.symbol
                .into_value(), self.venue.into_value(), self.currency.into_value(), self
                .side.into_value(), self.quantity_micros.into_value(), self
                .limit_price_micros.into_value(), self.execution_mode.into_value(), self
                .trading_day.into_value()
            ],
        )
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        use golem_wasm::analysis::analysed_type::field;
        golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord {
            fields: vec![
                field("proposal_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("strategy_id", < String as golem_wasm::IntoValue > ::get_type()),
                field("symbol", < String as golem_wasm::IntoValue > ::get_type()),
                field("venue", < String as golem_wasm::IntoValue > ::get_type()),
                field("currency", < String as golem_wasm::IntoValue > ::get_type()),
                field("side", < SideInput as golem_wasm::IntoValue > ::get_type()),
                field("quantity_micros", < u64 as golem_wasm::IntoValue > ::get_type()),
                field("limit_price_micros", < u64 as golem_wasm::IntoValue >
                ::get_type()), field("execution_mode", < ExecutionModeInput as
                golem_wasm::IntoValue > ::get_type()), field("trading_day", < i32 as
                golem_wasm::IntoValue > ::get_type())
            ],
            name: Some(stringify!(RiskProposalInput).to_string()),
            owner: None,
        })
    }
}
impl golem_wasm::FromValue for RiskProposalInput {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Record(mut fields) if fields.len() == 10usize => {
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
                Ok(Self {
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
                })
            }
            golem_wasm::Value::Record(fields) => {
                Err(
                    format!(
                        "Expected Record with {} fields, got {}", 10usize, fields.len()
                    ),
                )
            }
            _ => Err(format!("Expected Record value, got {:?}", value)),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RiskAgentError {
    NotConfigured(NotConfigured),
    InvalidConfiguration(InvalidConfiguration),
    ConfigurationConflict(ConfigurationConflict),
    InvalidPortfolio(InvalidPortfolio),
    AuthorityRejected(AuthorityRejected),
}
impl golem_wasm::IntoValue for RiskAgentError {
    fn into_value(self) -> golem_wasm::Value {
        match self {
            Self::NotConfigured(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 0u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::InvalidConfiguration(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 1u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::ConfigurationConflict(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 2u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::InvalidPortfolio(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 3u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
            Self::AuthorityRejected(value) => {
                golem_wasm::Value::Variant {
                    case_idx: 4u32,
                    case_value: Some(Box::new(value.into_value())),
                }
            }
        }
    }
    fn get_type() -> golem_wasm::analysis::AnalysedType {
        golem_wasm::analysis::AnalysedType::Variant(golem_wasm::analysis::TypeVariant {
            name: Some(stringify!(RiskAgentError).to_string()),
            owner: None,
            cases: vec![
                golem_wasm::analysis::NameOptionTypePair { name : "NotConfigured"
                .to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("NotConfigured".to_string()).clone(), owner : None.clone(),
                fields : vec![golem_wasm::analysis::NameTypePair { name : "detail"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "InvalidConfiguration".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("InvalidConfiguration".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "ConfigurationConflict".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("ConfigurationConflict".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "existing_fingerprint".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }, golem_wasm::analysis::NameTypePair { name : "proposed_fingerprint"
                .to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "InvalidPortfolio".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("InvalidPortfolio".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }, golem_wasm::analysis::NameOptionTypePair { name :
                "AuthorityRejected".to_string(), typ :
                Some(golem_wasm::analysis::AnalysedType::Record(golem_wasm::analysis::TypeRecord
                { name : Some("AuthorityRejected".to_string()).clone(), owner : None
                .clone(), fields : vec![golem_wasm::analysis::NameTypePair { name :
                "detail".to_string(), typ :
                golem_wasm::analysis::AnalysedType::Str(golem_wasm::analysis::TypeStr,),
                }], },)), }
            ],
        })
    }
}
impl golem_wasm::FromValue for RiskAgentError {
    fn from_value(value: golem_wasm::Value) -> Result<Self, String> {
        match value {
            golem_wasm::Value::Variant { case_idx, case_value } => {
                match case_idx {
                    0u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(NotConfigured)
                                )
                            })?;
                        Ok(
                            Self::NotConfigured(
                                <NotConfigured as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    1u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}",
                                    stringify!(InvalidConfiguration)
                                )
                            })?;
                        Ok(
                            Self::InvalidConfiguration(
                                <InvalidConfiguration as golem_wasm::FromValue>::from_value(
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
                                    stringify!(ConfigurationConflict)
                                )
                            })?;
                        Ok(
                            Self::ConfigurationConflict(
                                <ConfigurationConflict as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    3u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(InvalidPortfolio)
                                )
                            })?;
                        Ok(
                            Self::InvalidPortfolio(
                                <InvalidPortfolio as golem_wasm::FromValue>::from_value(
                                    *inner_value,
                                )?,
                            ),
                        )
                    }
                    4u32 => {
                        let inner_value = case_value
                            .ok_or_else(|| {
                                format!(
                                    "Expected case_value for {}", stringify!(AuthorityRejected)
                                )
                            })?;
                        Ok(
                            Self::AuthorityRejected(
                                <AuthorityRejected as golem_wasm::FromValue>::from_value(
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
