use super::*;

pub(super) fn reflected_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::SampledImage)
}

pub(super) fn reflected_storage_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageImage)
}

pub(super) fn reflected_buffer_read_names(reflection: &ShaderReflection) -> Vec<String> {
    // Include uniform buffers (always read-only) AND read-only storage buffers
    // (StructuredBuffer<T> reflected as StorageBuffer with Read access).
    if !reflection.parameters.is_empty() {
        return reflection
            .parameters
            .iter()
            .filter(|p| {
                matches!(p.kind, crate::ShaderParameterKind::Resource(core::BindingKind::UniformBuffer))
                    || (matches!(p.kind, crate::ShaderParameterKind::Resource(core::BindingKind::StorageBuffer))
                        && p.access == crate::ShaderResourceAccess::Read)
            })
            .map(|p| p.name.clone())
            .collect();
    }
    // Fallback: layout-only reflection has no access info — use UniformBuffer only.
    reflected_bindings_of_kind(reflection, core::BindingKind::UniformBuffer)
}

pub(super) fn reflected_buffer_write_names(reflection: &ShaderReflection) -> Vec<String> {
    // Only include storage buffers that are actually writable (RWStructuredBuffer<T>).
    // Read-only storage buffers (StructuredBuffer<T>) are excluded — they appear in
    // reflected_buffer_read_names instead to generate correct ShaderRead barriers.
    if !reflection.parameters.is_empty() {
        return reflection
            .parameters
            .iter()
            .filter(|p| {
                matches!(p.kind, crate::ShaderParameterKind::Resource(core::BindingKind::StorageBuffer))
                    && p.access != crate::ShaderResourceAccess::Read
            })
            .map(|p| p.name.clone())
            .collect();
    }
    // Fallback: layout-only reflection has no access info — treat all as writes (conservative).
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageBuffer)
}

/// Return the appropriate `RgState` for a named buffer read in a shader.
///
/// Uniform buffers get `UniformRead` (which maps to `UNIFORM_READ` access in Vulkan
/// barriers). Read-only storage buffers get `ShaderRead` (`SHADER_READ` access).
/// Falls back to `default_state` when the name isn't in the parameter list.
pub(super) fn reflected_read_state_for(
    reflection: &ShaderReflection,
    name: &str,
    default_state: RgState,
) -> RgState {
    for parameter in &reflection.parameters {
        if parameter.name != name {
            continue;
        }
        return match parameter.kind {
            crate::ShaderParameterKind::Resource(core::BindingKind::UniformBuffer) => {
                RgState::UniformRead
            }
            _ => RgState::ShaderRead,
        };
    }
    default_state
}

pub(super) fn reflected_push_constant_stages(
    reflection: &ShaderReflection,
    fallback: StageMask,
) -> StageMask {
    let mask = reflection.layout.push_constants_stage_mask;
    if mask.0 == 0 { fallback } else { mask }
}

pub(super) fn validate_typed_push_constants_size(
    reflection: &ShaderReflection,
    provided_bytes: usize,
    type_name: &str,
    shader_label: &str,
) -> Result<()> {
    let expected_bytes = reflection.layout.push_constants_bytes as usize;
    if expected_bytes == 0 || provided_bytes == expected_bytes {
        return Ok(());
    }

    Err(Error::InvalidInput(format!(
        "typed push constants `{type_name}` provide {provided_bytes} bytes for shader `{shader_label}`, but shader reflection declares {expected_bytes} bytes"
    )))
}

fn reflected_bindings_of_kind(
    reflection: &ShaderReflection,
    kind: core::BindingKind,
) -> Vec<String> {
    if !reflection.parameters.is_empty() {
        return reflection
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == crate::ShaderParameterKind::Resource(kind))
            .map(|parameter| parameter.name.clone())
            .collect();
    }

    reflection
        .layout
        .groups
        .iter()
        .flat_map(|group| group.bindings.iter())
        .filter(|binding| binding.kind == kind)
        .map(|binding| binding.path.clone())
        .collect()
}

/// Returns `(pool_bind_groups, push_descriptor_set)`.
///
/// `push_descriptor_set` is populated when the reflected layout contains a
/// descriptor group where every binding has `UpdateRate::Draw`.  These bindings
/// are excluded from the pool-allocated bind group and instead returned as a
/// `PushDescriptorSetDesc` that the caller attaches to `PassDesc::push_descriptor_set`.
/// The Vulkan backend will then push them inline via `vkCmdPushDescriptorSetKHR`.
pub(super) fn build_reflected_bind_group(
    engine: &Engine,
    layout_handle: core::PipelineLayoutHandle,
    reflection: &ShaderReflection,
    eager_bindings: &HashMap<String, ImageBinding>,
    images_by_name: &HashMap<String, GraphImageRecord>,
    samplers_by_name: &HashMap<String, core::SamplerHandle>,
    buffers_by_name: &HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
    eager_samplers: &HashMap<String, core::SamplerHandle>,
    eager_buffers: &HashMap<String, (core::BufferHandle, crate::BufferDesc)>,
    output_image: Option<(&str, ImageHandle)>,
    intra_frame_cache: Option<&mut HashMap<u64, core::BindGroupHandle>>,
) -> Result<(Vec<std::sync::Arc<BindGroup>>, Option<core::PushDescriptorSetDesc>)> {
    let has_bindings = reflection
        .layout
        .groups
        .iter()
        .any(|g| !g.bindings.is_empty());
    if !has_bindings {
        return Ok((Vec::new(), None));
    }

    let resolve_image = |path: &str| -> Option<ImageBinding> {
        eager_bindings.get(path).copied().or_else(|| {
            images_by_name.get(path).map(|r| ImageBinding {
                handle: r.handle,
                subresource: r.subresource,
            })
        })
    };
    let image_desc = |handle: ImageHandle| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == handle)
            .map(|record| record.desc)
    };

    let mut missing = Vec::new();
    let mut entries = Vec::new();
    let mut push_bindings: Vec<core::PushDescriptorBinding> = Vec::new();
    let mut push_set_index: Option<u32> = None;

    for (group_index, group) in reflection.layout.groups.iter().enumerate() {
        // All-Draw-rate groups route through push descriptors rather than pool allocation.
        let is_push_group = engine.caps().features.push_descriptors
            && !group.bindings.is_empty()
            && group
                .bindings
                .iter()
                .all(|b| b.update_rate == core::UpdateRate::Draw);

        for binding in &group.bindings {
            if is_push_group {
                // Build PushDescriptorBinding for this draw-rate binding.
                match binding.kind {
                    BindingKind::SampledImage => {
                        if let Some(image) = resolve_image(&binding.path) {
                            push_bindings.push(core::PushDescriptorBinding::SampledImage {
                                binding: binding.binding,
                                image_view: image.handle,
                                layout: crate::RgState::ShaderRead,
                            });
                            push_set_index = Some(group_index as u32);
                        } else {
                            missing.push(format!(
                                "{} ({:?} push set binding {})",
                                binding.path, binding.kind, binding.binding
                            ));
                        }
                    }
                    BindingKind::Sampler => {
                        let handle = eager_samplers
                            .get(&binding.path)
                            .or_else(|| samplers_by_name.get(&binding.path))
                            .copied()
                            .unwrap_or_else(|| engine.default_sampler());
                        push_bindings.push(core::PushDescriptorBinding::Sampler {
                            binding: binding.binding,
                            sampler: handle,
                        });
                        push_set_index = Some(group_index as u32);
                    }
                    BindingKind::StorageBuffer => {
                        if let Some((handle, desc)) = eager_buffers
                            .get(&binding.path)
                            .or_else(|| buffers_by_name.get(&binding.path))
                            .copied()
                            .or_else(|| engine.global_buffer(&binding.path))
                        {
                            push_bindings.push(core::PushDescriptorBinding::StorageBuffer {
                                binding: binding.binding,
                                buffer: handle,
                                offset: 0,
                                range: desc.size,
                            });
                            push_set_index = Some(group_index as u32);
                        } else {
                            missing.push(format!(
                                "{} ({:?} push set binding {})",
                                binding.path, binding.kind, binding.binding
                            ));
                        }
                    }
                    BindingKind::UniformBuffer => {
                        if let Some((handle, desc)) = eager_buffers
                            .get(&binding.path)
                            .or_else(|| buffers_by_name.get(&binding.path))
                            .copied()
                            .or_else(|| engine.global_buffer(&binding.path))
                        {
                            push_bindings.push(core::PushDescriptorBinding::UniformBuffer {
                                binding: binding.binding,
                                buffer: handle,
                                offset: 0,
                                range: desc.size,
                            });
                            push_set_index = Some(group_index as u32);
                        } else {
                            missing.push(format!(
                                "{} ({:?} push set binding {})",
                                binding.path, binding.kind, binding.binding
                            ));
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Regular pool-allocated path.
            match binding.kind {
                BindingKind::SampledImage => {
                    let image = resolve_image(&binding.path).or_else(|| {
                        engine.global_image(&binding.path).map(|(handle, _desc)| ImageBinding {
                            handle,
                            subresource: single_subresource(),
                        })
                    });
                    if let Some(image) = image {
                        if let Some(desc) = image_desc(image.handle) {
                            validate_reflected_image_usage(&binding.path, binding.kind, desc)?;
                        }
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::ImageView {
                                image: image.handle,
                                subresource: image.subresource,
                            },
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                BindingKind::StorageImage => {
                    let image = if let Some((name, h)) = output_image {
                        if binding.path == name {
                            Some(ImageBinding {
                                handle: h,
                                subresource: single_subresource(),
                            })
                        } else {
                            resolve_image(&binding.path)
                        }
                    } else {
                        resolve_image(&binding.path)
                    };
                    if let Some(image) = image {
                        if let Some(desc) = image_desc(image.handle) {
                            validate_reflected_image_usage(&binding.path, binding.kind, desc)?;
                        }
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::ImageView {
                                image: image.handle,
                                subresource: image.subresource,
                            },
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                BindingKind::Sampler => {
                    let handle = eager_samplers
                        .get(&binding.path)
                        .or_else(|| samplers_by_name.get(&binding.path))
                        .copied()
                        .unwrap_or_else(|| engine.default_sampler());
                    entries.push(BindGroupEntry {
                        path: binding.path.clone(),
                        resource: ResourceBinding::Sampler(handle),
                    });
                }
                BindingKind::StorageBuffer | BindingKind::UniformBuffer => {
                    if let Some((handle, desc)) = eager_buffers
                        .get(&binding.path)
                        .or_else(|| buffers_by_name.get(&binding.path))
                        .copied()
                        .or_else(|| engine.global_buffer(&binding.path))
                    {
                        validate_reflected_buffer_usage(&binding.path, binding.kind, desc)?;
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::Buffer(handle),
                        });
                    } else {
                        missing.push(format!(
                            "{} ({:?} set binding {})",
                            binding.path, binding.kind, binding.binding
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    if !missing.is_empty() {
        return Err(Error::InvalidInput(format!(
            "shader reflection requires unbound resources: {}",
            missing.join(", ")
        )));
    }

    let push_descriptor_set = push_set_index.map(|set| core::PushDescriptorSetDesc {
        set,
        bindings: push_bindings,
    });

    if entries.is_empty() {
        return Ok((Vec::new(), push_descriptor_set));
    }

    let desc = BindGroupDesc {
        layout: layout_handle,
        entries,
    };

    // Cross-frame bind group cache: compute a hash of (layout + all resource handles).
    // On hit: reuse the cached Arc<BindGroup> — zero vkAllocateDescriptorSets overhead.
    // On miss: allocate a new bind group, wrap in Arc, store in cache for future frames.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    desc.layout.hash(&mut hasher);
    for e in &desc.entries {
        e.path.hash(&mut hasher);
        match e.resource {
            ResourceBinding::Buffer(h) => h.hash(&mut hasher),
            ResourceBinding::Image(h) => h.hash(&mut hasher),
            ResourceBinding::Sampler(h) => h.hash(&mut hasher),
            ResourceBinding::ImageView { image, .. } => image.hash(&mut hasher),
            _ => {}
        }
    }
    let key = hasher.finish();

    // Check the intra-frame cache for duplicate detection.
    if let Some(cache) = intra_frame_cache {
        cache.insert(key, core::BindGroupHandle::default());
    }

    // Check the cross-frame cache in the Engine.
    {
        let current_frame = engine.bind_group_frame_index();
        let mut bg_cache = engine
            .bind_group_cache()
            .lock();
        if let Some(entry) = bg_cache.entries.get_mut(&key) {
            // Cache hit: update last-used frame and reuse the Arc.
            entry.last_used_frame = current_frame;
            let arc = std::sync::Arc::clone(&entry.bind_group);
            return Ok((vec![arc], push_descriptor_set));
        }
    }

    // Cache miss: allocate a new bind group, wrap in Arc, add to cross-frame cache.
    let bind_group = std::sync::Arc::new(engine.create_bind_group(desc)?);
    {
        let current_frame = engine.bind_group_frame_index();
        let mut bg_cache = engine
            .bind_group_cache()
            .lock();
        bg_cache.entries.insert(key, crate::engine::CachedBindGroupEntry {
            bind_group: std::sync::Arc::clone(&bind_group),
            last_used_frame: current_frame,
        });
    }
    Ok((vec![bind_group], push_descriptor_set))
}

pub(super) fn validate_pass_target_usage(
    name: &str,
    desc: ImageDesc,
    required: crate::ImageUsage,
) -> Result<()> {
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "pass target '{name}' requires {required:?} but image was created with {:?}",
        desc.usage
    )))
}

fn validate_reflected_image_usage(path: &str, kind: BindingKind, desc: ImageDesc) -> Result<()> {
    let required = match kind {
        BindingKind::SampledImage => crate::ImageUsage::SAMPLED,
        BindingKind::StorageImage => crate::ImageUsage::STORAGE,
        _ => return Ok(()),
    };
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "shader parameter '{path}' reflected as {kind:?}, but bound image usage {:?} does not include {:?}",
        desc.usage, required
    )))
}

fn validate_reflected_buffer_usage(path: &str, kind: BindingKind, desc: BufferDesc) -> Result<()> {
    let required = match kind {
        BindingKind::UniformBuffer => BufferUsage::UNIFORM,
        // Read-only storage buffers (StructuredBuffer<T>) still need STORAGE usage because
        // they are bound as STORAGE_BUFFER descriptor type even when accessed read-only.
        BindingKind::StorageBuffer => BufferUsage::STORAGE,
        _ => return Ok(()),
    };
    if desc.usage.contains(required) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "shader parameter '{path}' reflected as {kind:?}, but bound buffer usage {:?} does not include {:?}",
        desc.usage, required
    )))
}

pub(super) fn validate_deferred_reflected_resources(
    pass_name: &str,
    deferred: &DeferredPassResolve,
    images_by_name: &HashMap<String, GraphImageRecord>,
    buffers_by_name: &HashMap<String, (core::BufferHandle, BufferDesc)>,
) -> Vec<GraphDiagnostic> {
    let mut diagnostics = Vec::new();
    let image_desc_for = |binding: ImageBinding| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == binding.handle)
            .map(|record| record.desc)
    };
    let image_desc_for_output = |handle: ImageHandle| -> Option<ImageDesc> {
        images_by_name
            .values()
            .find(|record| record.handle == handle)
            .map(|record| record.desc)
    };

    for parameter in &deferred.reflection.parameters {
        let crate::ShaderParameterKind::Resource(kind) = parameter.kind else {
            continue;
        };
        match kind {
            BindingKind::SampledImage | BindingKind::StorageImage => {
                let binding = deferred
                    .eager_bindings
                    .get(&parameter.name)
                    .copied()
                    .or_else(|| {
                        images_by_name
                            .get(&parameter.name)
                            .map(|record| ImageBinding {
                                handle: record.handle,
                                subresource: record.subresource,
                            })
                    });
                let desc = binding.and_then(image_desc_for).or_else(|| {
                    deferred.storage_output.as_ref().and_then(|(name, handle)| {
                        (name == &parameter.name)
                            .then_some(*handle)
                            .and_then(image_desc_for_output)
                    })
                });
                let Some(desc) = desc else {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!(
                            "pass '{pass_name}' requires reflected image '{}' but no image with that name was bound",
                            parameter.name
                        ),
                    });
                    continue;
                };
                if let Err(error) = validate_reflected_image_usage(&parameter.name, kind, desc) {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!("pass '{pass_name}': {error}"),
                    });
                }
            }
            BindingKind::UniformBuffer | BindingKind::StorageBuffer => {
                let desc = deferred
                    .eager_buffers
                    .get(&parameter.name)
                    .or_else(|| buffers_by_name.get(&parameter.name))
                    .map(|(_, desc)| *desc);
                let Some(desc) = desc else {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!(
                            "pass '{pass_name}' requires reflected buffer '{}' but no buffer with that name was bound",
                            parameter.name
                        ),
                    });
                    continue;
                };
                if let Err(error) = validate_reflected_buffer_usage(&parameter.name, kind, desc) {
                    diagnostics.push(GraphDiagnostic {
                        level: DiagnosticLevel::Error,
                        message: format!("pass '{pass_name}': {error}"),
                    });
                }
            }
            BindingKind::Sampler | BindingKind::AccelerationStructure => {}
        }
    }
    diagnostics
}

pub(super) fn reflected_buffer_uses(
    reflection: &ShaderReflection,
    buffers_by_name: &HashMap<String, (core::BufferHandle, BufferDesc)>,
    eager_buffers: &HashMap<String, (core::BufferHandle, BufferDesc)>,
) -> Result<(Vec<crate::BufferUse>, Vec<crate::BufferUse>)> {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for parameter in &reflection.parameters {
        let crate::ShaderParameterKind::Resource(kind) = parameter.kind else {
            continue;
        };
        if !matches!(
            kind,
            BindingKind::UniformBuffer | BindingKind::StorageBuffer
        ) {
            continue;
        }
        let Some((handle, desc)) = eager_buffers
            .get(&parameter.name)
            .or_else(|| buffers_by_name.get(&parameter.name))
            .copied()
        else {
            return Err(Error::InvalidInput(format!(
                "shader reflection requires buffer '{}', but no buffer with that name was bound",
                parameter.name
            )));
        };
        validate_reflected_buffer_usage(&parameter.name, kind, desc)?;
        let read_state = match kind {
            BindingKind::UniformBuffer => RgState::UniformRead,
            BindingKind::StorageBuffer => RgState::ShaderRead,
            _ => RgState::ShaderRead,
        };
        let use_ = crate::BufferUse {
            buffer: handle,
            access: Access::Read,
            state: read_state,
            offset: 0,
            size: desc.size,
        };
        match parameter.access {
            crate::ShaderResourceAccess::Read => reads.push(use_),
            crate::ShaderResourceAccess::Write => writes.push(crate::BufferUse {
                access: Access::Write,
                state: RgState::ShaderWrite,
                ..use_
            }),
            crate::ShaderResourceAccess::ReadWrite => {
                reads.push(use_);
                writes.push(crate::BufferUse {
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    ..use_
                });
            }
        }
    }
    Ok((reads, writes))
}

pub(super) fn append_unique_buffer_uses(
    target: &mut Vec<crate::BufferUse>,
    uses: Vec<crate::BufferUse>,
) {
    for use_ in uses {
        if !target.iter().any(|existing| {
            existing.buffer == use_.buffer
                && existing.offset == use_.offset
                && existing.size == use_.size
                && existing.access == use_.access
        }) {
            target.push(use_);
        }
    }
}
