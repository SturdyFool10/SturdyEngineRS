// Tests extracted from crates/clay-ui/src/layout/shader.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn packs_uniforms_with_stable_offsets() {
    let packet = UiShaderUniformPacket::pack_push_constants(&[
        UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
        UiShaderUniform::new("offset", UiShaderUniformValue::Vec2([1.0, 2.0])),
        UiShaderUniform::new("enabled", UiShaderUniformValue::Bool(true)),
    ])
    .unwrap();

    assert_eq!(packet.entry("amount").unwrap().offset, 0);
    assert_eq!(packet.entry("offset").unwrap().offset, 4);
    assert_eq!(packet.entry("enabled").unwrap().offset, 12);
    assert_eq!(packet.bytes.len(), 16);
}

#[test]
fn rejects_duplicate_uniform_names() {
    let err = UiShaderUniformPacket::pack_push_constants(&[
        UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
        UiShaderUniform::new("amount", UiShaderUniformValue::Float(1.0)),
    ])
    .unwrap_err();

    assert_eq!(
        err,
        UiShaderUniformPackError::DuplicateUniform("amount".into())
    );
}

#[test]
fn rejects_push_constant_packets_over_limit() {
    let uniforms = (0..9)
        .map(|index| {
            UiShaderUniform::new(format!("v{index}"), UiShaderUniformValue::Vec4([0.0; 4]))
        })
        .collect::<Vec<_>>();

    let err = UiShaderUniformPacket::pack_push_constants(&uniforms).unwrap_err();

    assert_eq!(
        err,
        UiShaderUniformPackError::PushConstantLimitExceeded {
            size: 144,
            limit: UI_SHADER_PUSH_CONSTANT_LIMIT
        }
    );
}

#[test]
fn packs_parameter_batch_records_with_aligned_command_offsets() {
    let batch = UiShaderParameterBatch::pack_commands([
        (
            3,
            &[
                UiShaderUniform::new("amount", UiShaderUniformValue::Float(0.5)),
                UiShaderUniform::new("offset", UiShaderUniformValue::Vec2([1.0, 2.0])),
            ][..],
        ),
        (
            8,
            &[UiShaderUniform::new(
                "color",
                UiShaderUniformValue::Vec4([1.0, 0.0, 0.0, 1.0]),
            )][..],
        ),
    ])
    .unwrap();

    assert_eq!(batch.records.len(), 2);
    assert_eq!(batch.records[0].command_index, 3);
    assert_eq!(batch.records[0].offset, 0);
    assert_eq!(batch.records[0].size, 12);
    assert_eq!(batch.records[1].command_index, 8);
    assert_eq!(batch.records[1].offset, 16);
    assert_eq!(batch.records[1].size, 16);
    assert_eq!(batch.bytes.len(), 32);
}
