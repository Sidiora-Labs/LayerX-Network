use crate::{sources::CompleteSignatureSource, SignatureSource};
use anyhow::Context;
use ethabi::{Event, EventParam, ParamType, RawLog, Token};
use itertools::Itertools;
use sig_provider_proto::blockscout::sig_provider::v1::{Abi, Argument};
use std::{collections::HashSet, sync::Arc};

pub struct SourceAggregator {
    sources: Vec<Arc<dyn SignatureSource + Send + Sync + 'static>>,
    complete_sources: Vec<Arc<dyn CompleteSignatureSource + Send + Sync + 'static>>,
}

macro_rules! proxy {
    ($sources:expr, $request:expr, $fn:ident) => {{
        let tasks = $sources.iter().map(|source| source.$fn($request));
        let responses: Vec<_> = futures::future::join_all(tasks)
            .await
            .into_iter()
            .zip($sources.iter())
            .filter_map(|(resp, source)| match resp {
                Ok(resp) => Some(resp),
                Err(error) => {
                    tracing::error!(
                        "could not call {} for host {}, error: {}",
                        stringify!($fn),
                        source.source(),
                        error
                    );
                    None
                }
            })
            .collect();
        responses
    }};
}

macro_rules! get_event_signatures {
    ($sources:expr, $request:expr) => {{
        let responses = proxy!($sources, $request, get_event_signatures);
        crate::aggregator::SourceAggregator::merge_signatures(responses)
    }};
}

impl SourceAggregator {
    // You should provide sources in priority descending order (first - max priority)
    pub fn new(
        sources: Vec<Arc<dyn SignatureSource + Send + Sync + 'static>>,
        complete_sources: Vec<Arc<dyn CompleteSignatureSource + Send + Sync + 'static>>,
    ) -> SourceAggregator {
        SourceAggregator {
            sources,
            complete_sources,
        }
    }

    fn merge_signatures<T, I: IntoIterator<Item = T>, II: IntoIterator<Item = I>>(
        sigs: II,
    ) -> Vec<T>
    where
        T: Clone + Eq + std::hash::Hash,
    {
        let mut content: HashSet<T> = HashSet::default();
        sigs.into_iter()
            .flatten()
            .filter(|sig| content.insert(sig.clone()))
            .collect()
    }

    pub async fn create_signatures(&self, abi: &str) -> Result<(), anyhow::Error> {
        let sources = self.sources.clone();
        let _responses = proxy!(sources, abi, create_signatures);
        Ok(())
    }

    pub async fn get_function_signatures(&self, hex: &str) -> Result<Vec<String>, anyhow::Error> {
        let sources = &self.sources;
        let responses = proxy!(sources, hex, get_function_signatures);
        let signatures = Self::merge_signatures(responses);
        Ok(signatures)
    }

    pub async fn get_function_abi(&self, tx_input: &[u8]) -> Result<Vec<Abi>, anyhow::Error> {
        if tx_input.len() < 4 {
            anyhow::bail!("tx input len must be at least 4 bytes");
        }
        let hex_sig = hex::encode(&tx_input[..4]);
        let sigs = self.get_function_signatures(&hex_sig).await?;
        Ok(sigs
            .into_iter()
            .filter_map(|sig| {
                let (name, args) = parse_signature(&sig)?;
                let values = decode_txinput(&args, &tx_input[4..])?;
                let inputs = parse_args("arg".into(), &args, &values);
                Some(Abi {
                    name: name.into(),
                    inputs,
                })
            })
            .collect())
    }

    pub async fn get_event_abi(&self, raw: RawLog) -> Result<Vec<Abi>, anyhow::Error> {
        if raw.topics.is_empty() {
            anyhow::bail!("log should contain at least one topic");
        }
        let hex_sig = hex::encode(raw.topics[0].as_bytes());

        let complete_sigs = get_event_signatures!(&self.complete_sources, &hex_sig);
        let sigs = get_event_signatures!(&self.sources, &hex_sig);

        process_event_signatures(&raw, complete_sigs, sigs).await
    }

    pub async fn batch_get_event_abi(
        &self,
        raw_logs: Vec<RawLog>,
    ) -> Result<Vec<Vec<Abi>>, anyhow::Error> {
        let mut hex_sigs = Vec::new();
        for raw in &raw_logs {
            if raw.topics.is_empty() {
                anyhow::bail!("log should contain at least one topic")
            }
            hex_sigs.push(hex::encode(raw.topics[0].as_bytes()));
        }

        let complete_responses = proxy!(
            &self.complete_sources,
            &hex_sigs,
            batch_get_event_signatures
        );
        let responses = proxy!(&self.sources, &hex_sigs, batch_get_event_signatures);

        let mut results = Vec::new();
        for (index, raw_log) in raw_logs.iter().enumerate() {
            let batch_complete_signatures: Vec<_> = complete_responses
                .iter()
                .map(|response| response.get(index).cloned().unwrap_or_default())
                .collect();
            let complete_signatures = SourceAggregator::merge_signatures(batch_complete_signatures);

            let batch_signatures: Vec<_> = responses
                .iter()
                .map(|response| response.get(index).cloned().unwrap_or_default())
                .collect();
            let signatures = SourceAggregator::merge_signatures(batch_signatures);

            let abis = process_event_signatures(raw_log, complete_signatures, signatures).await?;
            results.push(abis)
        }

        Ok(results)
    }
}

async fn process_event_signatures(
    raw: &RawLog,
    complete_signatures: Vec<alloy_json_abi::Event>,
    signatures: Vec<String>,
) -> Result<Vec<Abi>, anyhow::Error> {
    let complete_abis: Vec<_> = complete_signatures.into_iter().filter_map(|alloy_event| {
        let ethabi_event = try_from_alloy_event_to_ethabi_event(alloy_event.clone())
            .map_err(|err| tracing::error!("converting alloy_json_abi::Event into ethabi::Event failed for {alloy_event:?}; err={err:#}")).ok()?;
        ethabi_event.parse_log_whole(raw.clone()).ok()
            .map(|event| {
                let mut names = Vec::new();
                let mut values = Vec::new();
                for param in event.params.into_iter() {
                    names.push(param.name);
                    values.push(param.value);
                }
                let indexed: Vec<_> = ethabi_event.inputs.iter().map(|param| param.indexed).collect();
                let args: Vec<_> = ethabi_event.inputs.into_iter().map(|param| param.kind).collect();
                let mut inputs = parse_args_with_names(&names, &args, &values);
                for (input, indexed) in inputs.iter_mut().zip(indexed) {
                    input.indexed = Some(indexed);
                }
                Abi {
                    name: ethabi_event.name,
                    inputs,
                }
            })
    }).collect();

    let abis: Vec<_> = signatures
        .into_iter()
        .filter_map(|sig| {
            let (name, args) = parse_signature(&sig)?;
            let (values, indexed) = decode_log(name.to_string(), &args, raw.clone())?;
            let mut inputs = parse_args("arg".into(), &args, &values);
            for (input, indexed) in inputs.iter_mut().zip(indexed) {
                input.indexed = Some(indexed);
            }
            Some(Abi {
                name: name.into(),
                inputs,
            })
        })
        .collect();

    let mut seen_abis = HashSet::new();
    let result: Vec<_> = complete_abis
        .into_iter()
        .chain(abis)
        .filter_map(|abi| {
            if !seen_abis.contains(&abi) {
                seen_abis.insert(abi.clone());
                Some(abi)
            } else {
                None
            }
        })
        .collect();
    Ok(result)
}

fn parse_signature(sig: &str) -> Option<(&str, Vec<ParamType>)> {
    let start = sig.find('(')?;
    let name = &sig[..start];
    let sig = &sig[start..];
    ethabi::param_type::Reader::read(sig)
        .ok()
        .and_then(|param| match param {
            ParamType::Tuple(params) => Some(params),
            _ => None,
        })
        .map(|params| (name, params))
}

fn decode_txinput(args: &[ParamType], tx_args: &[u8]) -> Option<Vec<Token>> {
    let decoded = ethabi::decode(args, tx_args).ok()?;

    // decode will not fail if it decodes only part of the input data
    // so we will encode the result and check, that we decoded the whole data
    let encoded = ethabi::encode(&decoded);
    if tx_args != encoded {
        return None;
    }
    Some(decoded)
}

fn decode_log(name: String, args: &[ParamType], raw: RawLog) -> Option<(Vec<Token>, Vec<bool>)> {
    const MAX_COMBINATIONS: usize = 10000;
    // because we don't know, which fields are indexed
    // we try to iterate over all possible combinations
    // and find whatever decodes without errors
    for (ind, indexes) in (0..args.len())
        .combinations(raw.topics.len() - 1)
        .enumerate()
    {
        if ind > MAX_COMBINATIONS {
            break;
        }
        let mut perm = vec![false; args.len()];
        for indexed in indexes {
            perm[indexed] = true;
        }
        let inputs: Vec<_> = args
            .iter()
            .zip(perm.iter())
            .enumerate()
            .map(|(ind, (param, indexed))| EventParam {
                name: format!("{ind}"),
                kind: param.clone(),
                indexed: *indexed,
            })
            .collect();
        let event = Event {
            name: name.clone(),
            inputs,
            anonymous: false,
        };
        let tokens = event
            .parse_log(raw.clone())
            .ok()
            .map(|log| log.params.into_iter().map(|param| param.value).collect());
        if let Some(tokens) = tokens {
            return Some((tokens, perm));
        }
    }
    None
}

fn parse_arg(name: String, param: &ParamType, value: &Token) -> Argument {
    let components = match (param, value) {
        (ParamType::Tuple(param), Token::Tuple(value)) => {
            parse_args(format!("{name}_"), param, value)
        }
        _ => Default::default(),
    };
    Argument {
        name,
        r#type: param.to_string(),
        components,
        indexed: None,
        value: value.to_string(),
    }
}

fn parse_args_with_names(names: &[String], args: &[ParamType], values: &[Token]) -> Vec<Argument> {
    let inputs = names
        .iter()
        .zip(args.iter())
        .zip(values.iter())
        .map(|((name, arg), value)| parse_arg(name.clone(), arg, value))
        .collect();
    inputs
}

fn parse_args(pref: String, args: &[ParamType], values: &[Token]) -> Vec<Argument> {
    let names = (0..args.len())
        .map(|index| format!("{pref}{index}"))
        .collect::<Vec<_>>();
    parse_args_with_names(&names, args, values)
}

fn try_from_alloy_event_to_ethabi_event(
    alloy_event: alloy_json_abi::Event,
) -> Result<ethabi::Event, anyhow::Error> {
    serde_json::from_value(
        serde_json::to_value(alloy_event).context("serializing alloy_json_abi::Event")?,
    )
    .context("deserializing ethabi::Event")
}

#[cfg(test)]
mod tests {
    use crate::sources::MockSignatureSource;

    use super::*;
    use ethabi::ethereum_types::{H160, H256, U256};
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn function() {
        let tests = vec![
            (
                "70a0823100000000000000000000000000000000219ab540356cbb839cbe05303d7705fa",
                Abi {
                    name: "balanceOf".into(),
                    inputs: vec![Argument {
                        name: "arg0".into(),
                        r#type: "address".into(),
                        components: vec![],
                        indexed: None,
                        value: "00000000219ab540356cbb839cbe05303d7705fa".into(),
                    }],
                },
            ),
            (
                "70a082310000000000000000000000000000000000000000000000000000000000bc61591234567812345678000000000000000000000000000000000000000000000000",
                Abi {
                    name: "branch_passphrase_public".into(),
                    inputs: vec![
                        Argument {
                            name: "arg0".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: None,
                            value: "bc6159".into(), // hex number 123456789
                        },
                        Argument {
                            name: "arg1".into(),
                            r#type: "bytes8".into(),
                            components: vec![],
                            indexed: None,
                            value: "1234567812345678".into(),
                        },
                    ],
                },
            ),
            (
                "70a082310000000000000000000000000000000000000000000000000000000000bc615900000000000000000000000000000000219ab540356cbb839cbe05303d7705fa",
                Abi {
                    name: "passphrase_calculate_transfer".into(),
                    inputs: vec![
                        Argument {
                            name: "arg0".into(),
                            r#type: "uint64".into(),
                            components: vec![],
                            indexed: None,
                            value: "bc6159".into(), // hex number 123456789
                        },
                        Argument {
                            name: "arg1".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: None,
                            value: "00000000219ab540356cbb839cbe05303d7705fa".into(),
                        },
                    ],
                },
            ),
        ];

        for (input, abi) in tests {
            let mut source = MockSignatureSource::new();
            source
                .expect_get_function_signatures()
                .with(mockall::predicate::eq("70a08231"))
                .times(1)
                .returning(|_| {
                    Ok(vec![
                        "balanceOf(address)".into(),
                        "branch_passphrase_public(uint256,bytes8)".into(),
                        "passphrase_calculate_transfer(uint64,address)".into(),
                    ])
                });
            let source = Arc::new(source);

            let agg = Arc::new(SourceAggregator::new(vec![source.clone()], vec![]));

            let function = agg
                .get_function_abi(&hex::decode(input).unwrap())
                .await
                .unwrap();
            assert_eq!(abi, function[0]);
        }
    }

    fn encode_tx_input_tuple() -> String {
        use ethabi::Token::*;
        let res = ethabi::encode(&[
            Tuple(vec![
                Uint(U256::from_dec_str("123456789").unwrap()),
                Address(H160(
                    hex::decode("00000000219ab540356cbb839cbe05303d7705fa")
                        .unwrap()
                        .try_into()
                        .unwrap(),
                )),
                Bytes(vec![123]),
            ]),
            Tuple(vec![
                Uint(U256::from_dec_str("123").unwrap()),
                FixedArray(vec![
                    FixedBytes(vec![11, 12, 13, 14]),
                    FixedBytes(vec![101, 102, 103, 104]),
                ]),
            ]),
        ]);
        hex::encode(res)
    }

    #[tokio::test]
    async fn function_tuple() {
        let encoded = encode_tx_input_tuple();
        let input = "68705463".to_string() + &encoded;

        let mut source = MockSignatureSource::new();
        source
            .expect_get_function_signatures()
            .with(mockall::predicate::eq("68705463"))
            .times(1)
            .returning(|_| {
                Ok(vec![
                    "test((uint256,address,bytes),(uint8,bytes32[2]))".into()
                ])
            });
        let source = Arc::new(source);

        let agg = Arc::new(SourceAggregator::new(vec![source.clone()], vec![]));

        let function = agg
            .get_function_abi(&hex::decode(input).unwrap())
            .await
            .unwrap();

        let expected = Abi {
            name: "test".into(),
            inputs: vec![
                Argument {
                    name: "arg0".into(),
                    r#type: "(uint256,address,bytes)".into(),
                    components: vec![
                        Argument {
                            name: "arg0_0".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: None,
                            value: "75bcd15".into(),
                        },
                        Argument {
                            name: "arg0_1".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: None,
                            value: "00000000219ab540356cbb839cbe05303d7705fa".into(),
                        },
                        Argument {
                            name: "arg0_2".into(),
                            r#type: "bytes".into(),
                            components: vec![],
                            indexed: None,
                            value: "7b".into(),
                        },
                    ],
                    indexed: None,
                    value: "(75bcd15,00000000219ab540356cbb839cbe05303d7705fa,7b)".into(),
                },
                Argument {
                    name: "arg1".into(),
                    r#type: "(uint8,bytes32[2])".into(),
                    components: vec![
                        Argument {
                            name: "arg1_0".into(),
                            r#type: "uint8".into(),
                            components: vec![],
                            indexed: None,
                            value: "7b".into(),
                        },
                        Argument {
                            name: "arg1_1".into(),
                            r#type: "bytes32[2]".into(),
                            components: vec![],
                            indexed: None,
                            value: "[0b0c0d0e00000000000000000000000000000000000000000000000000000000,6566676800000000000000000000000000000000000000000000000000000000]".into(),
                        },
                    ],
                    indexed: None,
                    value: "(7b,[0b0c0d0e00000000000000000000000000000000000000000000000000000000,6566676800000000000000000000000000000000000000000000000000000000])".into(),
                },
            ],
        };
        assert_eq!(expected, function[0]);
    }

    #[tokio::test]
    async fn event() {
        let tests = vec![
            (
                RawLog {
                    data: hex::decode(
                        "00000000000000000000000000000000000000000000000000000000006acfc0",
                    )
                    .unwrap(),
                    topics: vec![
                        H256::from_slice(
                            &hex::decode(
                                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                            )
                            .unwrap(),
                        ),
                        H256::from_slice(
                            &hex::decode(
                                "000000000000000000000000b8ace4d9bc469ddc8e788e636e817c299a1a8150",
                            )
                            .unwrap(),
                        ),
                        H256::from_slice(
                            &hex::decode(
                                "000000000000000000000000f76c5b19e86c256482f4aad1dae620a0c3ac0cd6",
                            )
                            .unwrap(),
                        ),
                    ],
                },
                "Transfer(address,address,uint256)",
                Abi {
                    name: "Transfer".into(),
                    inputs: vec![
                        Argument {
                            name: "arg0".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: Some(true),
                            value: "b8ace4d9bc469ddc8e788e636e817c299a1a8150".into(),
                        },
                        Argument {
                            name: "arg1".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: Some(true),
                            value: "f76c5b19e86c256482f4aad1dae620a0c3ac0cd6".into(),
                        },
                        Argument {
                            name: "arg2".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "6acfc0".into(),
                        },
                    ],
                },
            ),
            (
                RawLog {
                    data: hex::decode(
                        "0000000000000000000000000000000000000000000000083ed9ef578babdb5c00000000000000000000000000000000000000000000bf05c05e3ce0f57a7e39",
                    )
                    .unwrap(),
                    topics: vec![
                        H256::from_slice(
                            &hex::decode(
                                "1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1",
                            )
                            .unwrap(),
                        ),
                    ],
                },
                "Sync(uint112,uint112)",
                Abi {
                    name: "Sync".into(),
                    inputs: vec![
                        Argument {
                            name: "arg0".into(),
                            r#type: "uint112".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "83ed9ef578babdb5c".into(),
                        },
                        Argument {
                            name: "arg1".into(),
                            r#type: "uint112".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "bf05c05e3ce0f57a7e39".into(),
                        },
                    ],
                },
            ),
            (
                RawLog {
                    data: hex::decode(
                        "000000000000000000000000000000000000000000000000030d98d59a960000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000469ef5a473f28f3a21",
                    )
                    .unwrap(),
                    topics: vec![
                        H256::from_slice(
                            &hex::decode(
                                "d78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822",
                            )
                            .unwrap(),
                        ),
                        H256::from_slice(
                            &hex::decode(
                                "00000000000000000000000068b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                            )
                            .unwrap(),
                        ),
                        // this is actually last argument, but we can't determine that just from signature
                        // so we decode it as second argument
                        H256::from_slice(
                            &hex::decode(
                                "000000000000000000000000220575d6e7e8797ad18d0d660c7e1ecf4e1a1ed1",
                            )
                            .unwrap(),
                        ),
                    ],
                },
                "Swap(address,uint256,uint256,uint256,uint256,address)",
                Abi {
                    name: "Swap".into(),
                    inputs: vec![
                        Argument {
                            name: "arg0".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: Some(true),
                            value: "68b3465833fb72a70ecdf485e0e4c7bd8665fc45".into(),
                        },
                        Argument {
                            name: "arg1".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: Some(true),
                            value: "220575d6e7e8797ad18d0d660c7e1ecf4e1a1ed1".into(),
                        },
                        Argument {
                            name: "arg2".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "30d98d59a960000".into(),
                        },
                        Argument {
                            name: "arg3".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "0".into(),
                        },
                        Argument {
                            name: "arg4".into(),
                            r#type: "uint256".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "0".into(),
                        },
                        Argument {
                            name: "arg5".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: Some(false),
                            value: "0000000000000000000000469ef5a473f28f3a21".into(),
                        },
                    ],
                },
            ),
        ];
        for (input, sig, abi) in tests {
            let expected = hex::encode(input.topics[0].as_bytes());
            let mut source = MockSignatureSource::new();
            source
                .expect_get_event_signatures()
                .withf(move |hex| hex == expected)
                .times(1)
                .returning(|_| Ok(vec![sig.into()]));
            let source = Arc::new(source);

            let agg = Arc::new(SourceAggregator::new(vec![source.clone()], vec![]));

            let event = agg.get_event_abi(input).await.unwrap();
            assert_eq!(abi, event[0]);
        }
    }

    #[tokio::test]
    async fn event_dynamic() {
        let input = RawLog {
            data: hex::decode(
                "000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000097465737431323334350000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            topics: vec![
                H256::from_slice(
                    &hex::decode(
                        "74cb234c0dd0ccac09c19041a69978ccb865f1f44a2877a009549898f6395b10",
                    )
                    .unwrap(),
                ),
                H256::from_slice(
                    &hex::decode(
                        "2c3138faa13a5122f618ced1b0b4be95398183af357f6769e61ee2bccacc8b54",
                    )
                    .unwrap(),
                ),
            ],
        };
        let sig = "Test(string,string)";
        let abi = Abi {
            name: "Test".into(),
            inputs: vec![
                Argument {
                    name: "arg0".into(),
                    r#type: "string".into(),
                    components: vec![],
                    indexed: Some(true),
                    value: "2c3138faa13a5122f618ced1b0b4be95398183af357f6769e61ee2bccacc8b54"
                        .into(),
                },
                Argument {
                    name: "arg1".into(),
                    r#type: "string".into(),
                    components: vec![],
                    indexed: Some(false),
                    value: "test12345".into(),
                },
            ],
        };
        let expected = hex::encode(input.topics[0].as_bytes());
        let mut source = MockSignatureSource::new();
        source
            .expect_get_event_signatures()
            .withf(move |hex| hex == expected)
            .times(1)
            .returning(|_| Ok(vec![sig.into()]));
        let source = Arc::new(source);

        let agg = Arc::new(SourceAggregator::new(vec![source.clone()], vec![]));

        let event = agg.get_event_abi(input).await.unwrap();
        assert_eq!(abi, event[0]);
    }

    #[tokio::test]
    async fn event_tuple() {
        let input = RawLog {
            data: hex::decode("000000000000000000000000b8ace4d9bc469ddc8e788e636e817c299a1a8150000000000000000000000000f76c5b19e86c256482f4aad1dae620a0c3ac0cd6")
                .unwrap(),
            topics: vec![
                H256::from_slice(
                    &hex::decode(
                        "5db533d27f83c494aa583a6f8222343e612dd3efd69499ca6ae5dda6c6097df0",
                    )
                    .unwrap(),
                ),
                H256::from_slice(
                    &hex::decode(
                        "000000000000000000000000b8ace4d9bc469ddc8e788e636e817c299a1a8150",
                    )
                    .unwrap(),
                ),
            ],
        };
        let sig = "Test(address,(address,address))";
        let abi = Abi {
            name: "Test".into(),
            inputs: vec![
                Argument {
                    name: "arg0".into(),
                    r#type: "address".into(),
                    components: vec![],
                    indexed: Some(
                        true,
                    ),
                    value: "b8ace4d9bc469ddc8e788e636e817c299a1a8150".into(),
                },
                Argument {
                    name: "arg1".into(),
                    r#type: "(address,address)".into(),
                    components: vec![
                        Argument {
                            name: "arg1_0".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: None,
                            value: "b8ace4d9bc469ddc8e788e636e817c299a1a8150".into(),
                        },
                        Argument {
                            name: "arg1_1".into(),
                            r#type: "address".into(),
                            components: vec![],
                            indexed: None,
                            value: "f76c5b19e86c256482f4aad1dae620a0c3ac0cd6".into(),
                        },
                    ],
                    indexed: Some(
                        false,
                    ),
                    value: "(b8ace4d9bc469ddc8e788e636e817c299a1a8150,f76c5b19e86c256482f4aad1dae620a0c3ac0cd6)".into(),
                },
            ],
        };
        let expected = hex::encode(input.topics[0].as_bytes());
        let mut source = MockSignatureSource::new();
        source
            .expect_get_event_signatures()
            .withf(move |hex| hex == expected)
            .times(1)
            .returning(|_| Ok(vec![sig.into()]));
        let source = Arc::new(source);

        let agg = Arc::new(SourceAggregator::new(vec![source.clone()], vec![]));

        let event = agg.get_event_abi(input).await.unwrap();
        assert_eq!(abi, event[0]);
    }
}
