use crate::{
    Engine, GraphImage, Result, RuntimeApplyPath, RuntimeController, RuntimeSettingDescriptor,
    RuntimeSettingId, RuntimeSettingValue, ShaderProgram, ShellFrame,
};

const DEFAULT_SETTING_ID: &str = "debug.view_picker";
const OFF_VALUE: &str = "Off";

/// First-party helper for selecting and presenting one named runtime debug image.
pub struct DebugViewPicker {
    setting_id: String,
    passthrough: ShaderProgram,
}

impl DebugViewPicker {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            setting_id: DEFAULT_SETTING_ID.to_string(),
            passthrough: ShaderProgram::passthrough(engine)?,
        })
    }

    pub fn setting_id(&self) -> RuntimeSettingId {
        RuntimeSettingId::app(self.setting_id.clone())
    }

    pub fn register(&self, controller: &RuntimeController) -> Result<()> {
        let id = self.setting_id();
        if controller.setting_entry(id.clone()).is_none() {
            controller.register_app_setting(
                RuntimeSettingDescriptor::new(
                    id,
                    "Debug View Picker",
                    RuntimeApplyPath::Immediate,
                    OFF_VALUE,
                )
                .with_description(
                    "Choose a registered runtime debug image to present instead of the final scene.",
                ),
            )?;
        }
        Ok(())
    }

    pub fn selected_name(&self, controller: &RuntimeController) -> Option<String> {
        let value = controller.text_setting(self.setting_id())?;
        if value == OFF_VALUE || value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    pub fn set_selected_name(
        &self,
        controller: &mut RuntimeController,
        name: Option<&str>,
    ) -> Result<()> {
        controller
            .transact()
            .set_app_value(&self.setting_id, name.unwrap_or(OFF_VALUE))
            .apply()?;
        Ok(())
    }

    pub fn cycle_next(
        &self,
        controller: &mut RuntimeController,
        names: &[String],
    ) -> Result<Option<String>> {
        self.cycle(controller, names, 1)
    }

    pub fn cycle_previous(
        &self,
        controller: &mut RuntimeController,
        names: &[String],
    ) -> Result<Option<String>> {
        self.cycle(controller, names, -1)
    }

    pub fn present_selected(
        &self,
        shell_frame: &ShellFrame<'_>,
        target: &GraphImage,
    ) -> Result<bool> {
        let Some(name) = self.selected_name(&shell_frame.runtime_controller()) else {
            return Ok(false);
        };
        let available_names = shell_frame.debug_image_names();
        if !available_names.iter().any(|entry| entry == &name) {
            return Ok(false);
        }
        let Some(image) = shell_frame.inner().find_image_by_name(&name) else {
            return Ok(false);
        };
        target.blit_from(&image, &self.passthrough)?;
        Ok(true)
    }

    fn cycle(
        &self,
        controller: &mut RuntimeController,
        names: &[String],
        direction: isize,
    ) -> Result<Option<String>> {
        let mut entries = Vec::with_capacity(names.len() + 1);
        entries.push(OFF_VALUE.to_string());
        entries.extend(names.iter().cloned());

        let current = controller
            .setting_value(self.setting_id())
            .unwrap_or_else(|| RuntimeSettingValue::Text(OFF_VALUE.to_string()));
        let current_name = match current {
            RuntimeSettingValue::Text(value) => value,
            _ => OFF_VALUE.to_string(),
        };
        let current_index = entries
            .iter()
            .position(|entry| entry == &current_name)
            .unwrap_or(0) as isize;
        let len = entries.len() as isize;
        let next_index = (current_index + direction).rem_euclid(len) as usize;
        let next = entries[next_index].clone();
        self.set_selected_name(
            controller,
            if next == OFF_VALUE {
                None
            } else {
                Some(next.as_str())
            },
        )?;
        Ok((next != OFF_VALUE).then_some(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_cycles_through_debug_image_names() {
        let engine = Engine::with_backend(crate::BackendKind::Null).unwrap();
        let picker = DebugViewPicker::new(&engine).unwrap();
        let mut controller = RuntimeController::default();
        picker.register(&controller).unwrap();

        let names = vec!["motion_debug_view".to_string(), "hdr_composite".to_string()];
        assert_eq!(
            picker.cycle_next(&mut controller, &names).unwrap(),
            Some("motion_debug_view".to_string())
        );
        assert_eq!(
            picker.cycle_next(&mut controller, &names).unwrap(),
            Some("hdr_composite".to_string())
        );
        assert_eq!(picker.cycle_next(&mut controller, &names).unwrap(), None);
    }
}
