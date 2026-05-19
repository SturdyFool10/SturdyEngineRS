// Tests extracted from crates/sturdy-engine-core/src/backend/vulkan/mod.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn reflex_mode_encoding_round_trips_known_modes() {
    assert_eq!(
        decode_reflex_mode(encode_reflex_mode(ReflexMode::Off)),
        ReflexMode::Off
    );
    assert_eq!(
        decode_reflex_mode(encode_reflex_mode(ReflexMode::On)),
        ReflexMode::On
    );
    assert_eq!(
        decode_reflex_mode(encode_reflex_mode(ReflexMode::OnPlusBoost)),
        ReflexMode::OnPlusBoost
    );
}

#[test]
fn reflex_mode_decoding_treats_unknown_values_as_off() {
    assert_eq!(decode_reflex_mode(99), ReflexMode::Off);
}

#[test]
fn anti_lag_mode_encoding_round_trips_known_modes() {
    assert_eq!(
        decode_anti_lag_mode(encode_anti_lag_mode(AntiLagMode::Off)),
        AntiLagMode::Off
    );
    assert_eq!(
        decode_anti_lag_mode(encode_anti_lag_mode(AntiLagMode::On)),
        AntiLagMode::On
    );
}

#[test]
fn anti_lag_mode_decoding_treats_unknown_values_as_off() {
    assert_eq!(decode_anti_lag_mode(99), AntiLagMode::Off);
}

#[test]
fn anti_lag_vk_mode_matches_amd_values() {
    assert_eq!(vk_anti_lag_mode(AntiLagMode::Off) as i32, 2);
    assert_eq!(vk_anti_lag_mode(AntiLagMode::On) as i32, 1);
}

#[test]
fn optical_flow_grid_size_maps_supported_values() {
    assert_eq!(
        vk_optical_flow_grid_size(1).unwrap().as_raw(),
        vk::OpticalFlowGridSizeFlagsNV::TYPE_1X1.as_raw()
    );
    assert_eq!(
        vk_optical_flow_grid_size(4).unwrap().as_raw(),
        vk::OpticalFlowGridSizeFlagsNV::TYPE_4X4.as_raw()
    );
}

#[test]
fn optical_flow_grid_size_rejects_unknown_values() {
    assert!(matches!(
        vk_optical_flow_grid_size(3),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn compaction_build_sizes_use_source_size_without_scratch() {
    assert_eq!(
        compact_acceleration_structure_build_sizes(
            AccelerationStructureKind::BottomLevel,
            4096,
            AccelerationStructureKind::BottomLevel,
        )
        .unwrap(),
        AccelerationStructureBuildSizes {
            acceleration_structure_size: 4096,
            build_scratch_size: 0,
            update_scratch_size: 0,
        }
    );
}

#[test]
fn compaction_build_sizes_reject_wrong_source_kind() {
    let err = compact_acceleration_structure_build_sizes(
        AccelerationStructureKind::TopLevel,
        4096,
        AccelerationStructureKind::BottomLevel,
    )
    .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(message) if message.contains("source kind")));
}

#[test]
fn enabled_extension_matches_exact_extension_name() {
    let enabled = vec![
        "VK_KHR_video_queue".to_string(),
        "VK_KHR_video_decode_h264_extra".to_string(),
    ];

    assert!(enabled_extension(&enabled, "VK_KHR_video_queue"));
    assert!(!enabled_extension(&enabled, "VK_KHR_video_decode_h264"));
}

#[test]
fn device_fault_report_includes_description_addresses_and_vendor_info() {
    let fault_description = std::ffi::CString::new("fault in test pass").unwrap();
    let fault_info = vk::DeviceFaultInfoEXT::default()
        .description(&fault_description)
        .unwrap();
    let address_info = vk::DeviceFaultAddressInfoEXT::default()
        .address_type(vk::DeviceFaultAddressTypeEXT::WRITE_INVALID)
        .reported_address(0xabc0)
        .address_precision(64);
    let vendor_description = std::ffi::CString::new("vendor detail").unwrap();
    let vendor_info = vk::DeviceFaultVendorInfoEXT::default()
        .description(&vendor_description)
        .unwrap()
        .vendor_fault_code(0x12)
        .vendor_fault_data(0x34);

    let report = format_device_fault_info(
        "device lost",
        &fault_info,
        &[address_info],
        &[vendor_info],
        8,
    );

    assert!(report.contains("[device_fault] fault in test pass"));
    assert!(report.contains("[device_fault address info]"));
    assert!(report.contains("type=write_invalid address=0xabc0 precision=64"));
    assert!(report.contains("[device_fault vendor info]"));
    assert!(report.contains("code=0x12 data=0x34 desc=vendor detail"));
    assert!(report.contains("[device_fault vendor binary] size=8 bytes"));
}
