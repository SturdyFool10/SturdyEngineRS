use super::*;

pub(super) fn reflected_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::SampledImage)
}

pub(super) fn reflected_storage_image_reads(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageImage)
}

pub(super) fn reflected_buffer_read_names(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::UniformBuffer)
}

pub(super) fn reflected_buffer_write_names(reflection: &ShaderReflection) -> Vec<String> {
    reflected_bindings_of_kind(reflection, core::BindingKind::StorageBuffer)
}

pub(super) fn reflected_push_constant_stages(
    reflection: &ShaderReflection,
    fallback: StageMask,
) -> StageMask {
    let mask = reflection.layout.push_constants_stage_mask;
    if mask.0 == 0 { fallback } else { mask }
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
) -> Result<Vec<BindGroup>> {
    let has_bindings = reflection
        .layout
        .groups
        .iter()
        .any(|g| !g.bindings.is_empty());
    if !has_bindings {
        return Ok(Vec::new());
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
    for group in &reflection.layout.groups {
        for binding in &group.bindings {
            match binding.kind {
                BindingKind::SampledImage => {
                    if let Some(image) = resolve_image(&binding.path) {
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
                    if let Some((handle, _)) = eager_buffers
                        .get(&binding.path)
                        .or_else(|| buffers_by_name.get(&binding.path))
                    {
                        let (_, desc) = eager_buffers
                            .get(&binding.path)
                            .or_else(|| buffers_by_name.get(&binding.path))
                            //panic allowed, reason = "same lookup was proven Some immediately above"
                            .expect("buffer desc present with buffer handle");
                        validate_reflected_buffer_usage(&binding.path, binding.kind, *desc)?;
                        entries.push(BindGroupEntry {
                            path: binding.path.clone(),
                            resource: ResourceBinding::Buffer(*handle),
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

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let bind_group = engine.create_bind_group(BindGroupDesc {
        layout: layout_handle,
        entries,
    })?;
    Ok(vec![bind_group])
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
