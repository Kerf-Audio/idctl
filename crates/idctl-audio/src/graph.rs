//! `pw-dump` output: the interface's device and the nodes that belong to it.

use serde_json::Value;

/// One streaming direction of the interface.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub capture: bool,
    pub running: bool,
    /// ALSA period size in frames; 0 if PipeWire does not report one.
    pub period: u32,
    /// Extra frames kept in the device buffer; 0 if not reported.
    pub headroom: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub device_name: String,
    /// Active card profile, empty if unknown.
    pub profile: String,
    pub nodes: Vec<Node>,
}

fn props(object: &Value) -> &Value {
    &object["info"]["props"]
}

fn is_type(object: &Value, kind: &str) -> bool {
    object["type"].as_str() == Some(kind)
}

/// A USB id as PipeWire prints it: `"0x2708"`.
fn usb_id(value: &Value) -> Option<u16> {
    u16::from_str_radix(value.as_str()?.trim_start_matches("0x"), 16).ok()
}

/// A value from a node's flat `Props.params` list of alternating keys and
/// values.
fn alsa_prop(node: &Value, key: &str) -> u32 {
    let lists = node["info"]["params"]["Props"].as_array();
    lists
        .into_iter()
        .flatten()
        .filter_map(|p| p["params"].as_array())
        .find_map(|list| {
            let at = list.iter().position(|v| v.as_str() == Some(key))?;
            list.get(at + 1)?.as_u64()
        })
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}

/// The device with this USB vendor and product id, and its nodes.
pub fn interface(dump: &str, vendor: u16, product: u16) -> Result<Option<Interface>, String> {
    let dump: Value = serde_json::from_str(dump).map_err(|e| format!("pw-dump: {e}"))?;
    let objects = dump.as_array().map(Vec::as_slice).unwrap_or_default();
    let Some(device) = objects.iter().find(|o| {
        is_type(o, "PipeWire:Interface:Device")
            && usb_id(&props(o)["device.vendor.id"]) == Some(vendor)
            && usb_id(&props(o)["device.product.id"]) == Some(product)
    }) else {
        return Ok(None);
    };
    let nodes = objects
        .iter()
        .filter(|o| is_type(o, "PipeWire:Interface:Node"))
        .filter(|o| device["id"].is_u64() && props(o)["device.id"] == device["id"])
        .filter_map(|o| {
            Some(Node {
                id: u32::try_from(o["id"].as_u64()?).ok()?,
                name: props(o)["node.name"].as_str()?.to_owned(),
                capture: props(o)["media.class"].as_str() == Some("Audio/Source"),
                running: o["info"]["state"].as_str() == Some("running"),
                period: alsa_prop(o, "api.alsa.period-size"),
                headroom: alsa_prop(o, "api.alsa.headroom"),
            })
        })
        .collect();
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    Ok(Some(Interface {
        device_name: text(&props(device)["device.name"]),
        profile: text(&device["info"]["params"]["Profile"][0]["name"]),
        nodes,
    }))
}

/// Names of streams that only listen to an output (level meters,
/// visualisers). They cannot glitch what is heard, however late they run.
pub fn listeners(dump: &str) -> Vec<String> {
    let dump: Value = serde_json::from_str(dump).unwrap_or_default();
    let objects = dump.as_array().map(Vec::as_slice).unwrap_or_default();
    objects
        .iter()
        .filter(|o| is_type(o, "PipeWire:Interface:Node"))
        .filter(|o| props(o)["stream.monitor"].as_bool() == Some(true))
        .filter_map(|o| Some(props(o)["node.name"].as_str()?.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT_ONLY: &str = include_str!("../fixtures/dump-output-only.json");
    const PRO_AUDIO: &str = include_str!("../fixtures/dump-pro-audio.json");

    #[test]
    fn finds_the_interface_and_its_nodes() {
        let i = interface(OUTPUT_ONLY, 0x2708, 0x0016).unwrap().unwrap();
        assert_eq!(i.profile, "output:multichannel-output");
        assert!(i.device_name.starts_with("alsa_card.usb-"));
        assert_eq!(i.nodes.len(), 1);
        let n = &i.nodes[0];
        assert_eq!(n.id, 44);
        assert!(n.name.ends_with(".multichannel-output"));
        assert!(!n.capture);
        assert!(!n.running);
        assert_eq!((n.period, n.headroom), (32, 32));
    }

    #[test]
    fn pro_audio_has_both_directions() {
        let i = interface(PRO_AUDIO, 0x2708, 0x0016).unwrap().unwrap();
        assert_eq!(i.profile, "pro-audio");
        assert_eq!(i.nodes.len(), 2);
        let source = i.nodes.iter().find(|n| n.capture).unwrap();
        assert_eq!((source.id, source.period, source.headroom), (46, 64, 16));
        assert!(i.nodes.iter().all(|n| n.running));
    }

    #[test]
    fn another_vendor_is_not_ours() {
        assert_eq!(interface(OUTPUT_ONLY, 0x1235, 0x0016), Ok(None));
        assert_eq!(interface(OUTPUT_ONLY, 0x2708, 0x0001), Ok(None));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(interface("not json", 0x2708, 0x0016).is_err());
        assert_eq!(interface("[]", 0x2708, 0x0016), Ok(None));
        assert_eq!(interface("{}", 0x2708, 0x0016), Ok(None));
        let bare = r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{}},
            {"id":2,"type":"PipeWire:Interface:Device"},
            {"id":3,"type":"PipeWire:Interface:Device","info":{"props":
              {"device.vendor.id":"0x2708","device.product.id":"0x0016"}}}]"#;
        let i = interface(bare, 0x2708, 0x0016).unwrap().unwrap();
        assert!(i.nodes.is_empty());
        assert_eq!(i.profile, "");
    }

    #[test]
    fn monitor_streams_are_listeners() {
        let dump = r#"[
            {"id":142,"type":"PipeWire:Interface:Node","info":{"props":
              {"node.name":"quickshell","media.class":"Stream/Input/Audio","stream.monitor":true}}},
            {"id":150,"type":"PipeWire:Interface:Node","info":{"props":
              {"node.name":"My DAW","media.class":"Stream/Input/Audio"}}},
            {"id":151,"type":"PipeWire:Interface:Node","info":{"props":
              {"node.name":"Player","media.class":"Stream/Output/Audio"}}}]"#;
        assert_eq!(listeners(dump), ["quickshell"]);
        assert!(listeners("not json").is_empty());
    }
}
