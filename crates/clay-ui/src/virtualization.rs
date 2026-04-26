use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualListConfig {
    pub item_count: usize,
    pub item_extent: f32,
    pub viewport_extent: f32,
    pub scroll_offset: f32,
    pub overscan_items: usize,
}

impl VirtualListConfig {
    pub fn new(
        item_count: usize,
        item_extent: f32,
        viewport_extent: f32,
        scroll_offset: f32,
    ) -> Self {
        Self {
            item_count,
            item_extent,
            viewport_extent,
            scroll_offset,
            overscan_items: 1,
        }
    }

    pub fn overscan_items(mut self, overscan_items: usize) -> Self {
        self.overscan_items = overscan_items;
        self
    }

    pub fn layout(self) -> VirtualListLayout {
        if self.item_count == 0
            || self.item_extent <= f32::EPSILON
            || self.viewport_extent <= f32::EPSILON
        {
            return VirtualListLayout {
                item_count: self.item_count,
                item_extent: self.item_extent.max(0.0),
                viewport_extent: self.viewport_extent.max(0.0),
                total_extent: 0.0,
                max_scroll_offset: 0.0,
                scroll_offset: 0.0,
                visible_range: 0..0,
                render_range: 0..0,
                before_extent: 0.0,
                after_extent: 0.0,
            };
        }

        let total_extent = self.item_count as f32 * self.item_extent;
        let max_scroll_offset = (total_extent - self.viewport_extent).max(0.0);
        let scroll_offset = self.scroll_offset.clamp(0.0, max_scroll_offset);
        let visible_start = (scroll_offset / self.item_extent).floor() as usize;
        let visible_end =
            ((scroll_offset + self.viewport_extent) / self.item_extent).ceil() as usize;
        let visible_range = visible_start.min(self.item_count)..visible_end.min(self.item_count);
        let render_range = visible_range.start.saturating_sub(self.overscan_items)
            ..visible_range
                .end
                .saturating_add(self.overscan_items)
                .min(self.item_count);
        let before_extent = render_range.start as f32 * self.item_extent;
        let after_extent = (self.item_count - render_range.end) as f32 * self.item_extent;

        VirtualListLayout {
            item_count: self.item_count,
            item_extent: self.item_extent,
            viewport_extent: self.viewport_extent,
            total_extent,
            max_scroll_offset,
            scroll_offset,
            visible_range,
            render_range,
            before_extent,
            after_extent,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListLayout {
    pub item_count: usize,
    pub item_extent: f32,
    pub viewport_extent: f32,
    pub total_extent: f32,
    pub max_scroll_offset: f32,
    pub scroll_offset: f32,
    pub visible_range: Range<usize>,
    pub render_range: Range<usize>,
    pub before_extent: f32,
    pub after_extent: f32,
}

impl VirtualListLayout {
    pub fn is_empty(&self) -> bool {
        self.render_range.is_empty()
    }

    pub fn render_count(&self) -> usize {
        self.render_range.len()
    }

    pub fn item_offset(&self, index: usize) -> Option<f32> {
        (index < self.item_count).then_some(index as f32 * self.item_extent)
    }

    pub fn render_items(&self) -> impl Iterator<Item = VirtualItem> + '_ {
        self.render_range.clone().map(|index| VirtualItem {
            index,
            offset: index as f32 * self.item_extent,
            extent: self.item_extent,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualItem {
    pub index: usize,
    pub offset: f32,
    pub extent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_list_returns_visible_and_overscanned_ranges() {
        let layout = VirtualListConfig::new(100, 20.0, 100.0, 45.0)
            .overscan_items(2)
            .layout();

        assert_eq!(layout.visible_range, 2..8);
        assert_eq!(layout.render_range, 0..10);
        assert_eq!(layout.before_extent, 0.0);
        assert_eq!(layout.after_extent, 1800.0);
        assert_eq!(layout.render_count(), 10);
    }

    #[test]
    fn virtual_list_clamps_scroll_to_content_bounds() {
        let layout = VirtualListConfig::new(10, 12.0, 48.0, 500.0)
            .overscan_items(1)
            .layout();

        assert_eq!(layout.max_scroll_offset, 72.0);
        assert_eq!(layout.scroll_offset, 72.0);
        assert_eq!(layout.visible_range, 6..10);
        assert_eq!(layout.render_range, 5..10);
        assert_eq!(layout.before_extent, 60.0);
        assert_eq!(layout.after_extent, 0.0);
    }

    #[test]
    fn virtual_list_handles_empty_or_invalid_inputs() {
        let empty = VirtualListConfig::new(0, 20.0, 100.0, 0.0).layout();
        let invalid_extent = VirtualListConfig::new(10, 0.0, 100.0, 0.0).layout();
        let invalid_viewport = VirtualListConfig::new(10, 20.0, 0.0, 0.0).layout();

        assert!(empty.is_empty());
        assert!(invalid_extent.is_empty());
        assert!(invalid_viewport.is_empty());
    }

    #[test]
    fn virtual_list_items_report_absolute_offsets() {
        let layout = VirtualListConfig::new(20, 16.0, 64.0, 48.0)
            .overscan_items(1)
            .layout();

        let items = layout.render_items().collect::<Vec<_>>();

        assert_eq!(layout.visible_range, 3..7);
        assert_eq!(
            items.first().copied(),
            Some(VirtualItem {
                index: 2,
                offset: 32.0,
                extent: 16.0,
            })
        );
        assert_eq!(layout.item_offset(6), Some(96.0));
        assert_eq!(layout.item_offset(20), None);
    }
}
