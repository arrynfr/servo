/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Layout for elements with a CSS `display` property of `list-item`. These elements consist of a
//! block and an extra inline fragment for the marker.

use app_units::Au;
use euclid::default::Point2D;
use style::computed_values::list_style_type::T as ListStyleType;
use style::computed_values::position::T as Position;
use style::logical_geometry::LogicalSize;
use style::properties::ComputedValues;
use style::servo::restyle_damage::ServoRestyleDamage;

use crate::block::BlockFlow;
use crate::context::{with_thread_local_font_context, LayoutContext};
use crate::display_list::items::DisplayListSection;
use crate::display_list::{
    BorderPaintingMode, DisplayListBuildState, StackingContextCollectionState,
};
use crate::floats::FloatKind;
use crate::flow::{Flow, FlowClass, OpaqueFlow};
use crate::fragment::{
    CoordinateSystem, Fragment, FragmentBorderBoxIterator, GeneratedContentInfo, Overflow,
};
use crate::generated_content;
use crate::inline::InlineFlow;

#[allow(unsafe_code)]
unsafe impl crate::flow::HasBaseFlow for ListItemFlow {}

/// A block with the CSS `display` property equal to `list-item`.
#[derive(Debug)]
#[repr(C)]
pub struct ListItemFlow {
    /// Data common to all block flows.
    pub block_flow: BlockFlow,
    /// The marker, if outside. (Markers that are inside are instead just fragments on the interior
    /// `InlineFlow`.)
    pub marker_fragments: Vec<Fragment>,
}

impl ListItemFlow {
    pub fn from_fragments_and_flotation(
        main_fragment: Fragment,
        marker_fragments: Vec<Fragment>,
        flotation: Option<FloatKind>,
    ) -> ListItemFlow {
        let mut this = ListItemFlow {
            block_flow: BlockFlow::from_fragment_and_float_kind(main_fragment, flotation),
            marker_fragments: marker_fragments,
        };

        if let Some(ref marker) = this.marker_fragments.first() {
            match marker.style().get_list().list_style_type {
                ListStyleType::Disc |
                ListStyleType::None |
                ListStyleType::Circle |
                ListStyleType::Square |
                ListStyleType::DisclosureOpen |
                ListStyleType::DisclosureClosed => {},
                _ => this
                    .block_flow
                    .base
                    .restyle_damage
                    .insert(ServoRestyleDamage::RESOLVE_GENERATED_CONTENT),
            }
        }

        this
    }

    /// Assign inline size and position for the marker. This is done during the `assign_block_size`
    /// traversal because floats will impact the marker position. Therefore we need to have already
    /// called `assign_block_size` on the list item's block flow, in order to know which floats
    /// impact the position.
    ///
    /// Per CSS 2.1 § 12.5.1, the marker position is not precisely specified, but it must be on the
    /// left side of the content (for ltr direction). However, flowing the marker around floats
    /// matches the rendering of Gecko and Blink.
    fn assign_marker_inline_sizes(&mut self, layout_context: &LayoutContext) {
        let base = &self.block_flow.base;
        let available_rect = base.floats.available_rect(
            -base.position.size.block,
            base.position.size.block,
            base.block_container_inline_size,
        );
        let mut marker_inline_start = available_rect
            .unwrap_or(self.block_flow.fragment.border_box)
            .start
            .i;

        for marker in self.marker_fragments.iter_mut().rev() {
            let container_block_size = self
                .block_flow
                .explicit_block_containing_size(layout_context.shared_context());
            marker.assign_replaced_inline_size_if_necessary(
                base.block_container_inline_size,
                container_block_size,
            );

            // Do this now. There's no need to do this in bubble-widths, since markers do not
            // contribute to the inline size of this flow.
            let intrinsic_inline_sizes = marker.compute_intrinsic_inline_sizes();

            marker.border_box.size.inline = intrinsic_inline_sizes
                .content_intrinsic_sizes
                .preferred_inline_size;
            marker_inline_start = marker_inline_start - marker.border_box.size.inline;
            marker.border_box.start.i = marker_inline_start;
        }
    }

    fn assign_marker_block_sizes(&mut self, layout_context: &LayoutContext) {
        // FIXME(pcwalton): Do this during flow construction, like `InlineFlow` does?
        let marker_line_metrics = with_thread_local_font_context(layout_context, |font_context| {
            InlineFlow::minimum_line_metrics_for_fragments(
                &self.marker_fragments,
                font_context,
                &*self.block_flow.fragment.style,
            )
        });

        for marker in &mut self.marker_fragments {
            marker.assign_replaced_block_size_if_necessary();
            let marker_inline_metrics = marker.aligned_inline_metrics(
                layout_context,
                &marker_line_metrics,
                Some(&marker_line_metrics),
            );
            marker.border_box.start.b =
                marker_line_metrics.space_above_baseline - marker_inline_metrics.ascent;
        }
    }
}

impl Flow for ListItemFlow {
    fn class(&self) -> FlowClass {
        FlowClass::ListItem
    }

    fn as_mut_block(&mut self) -> &mut BlockFlow {
        &mut self.block_flow
    }

    fn as_block(&self) -> &BlockFlow {
        &self.block_flow
    }

    fn bubble_inline_sizes(&mut self) {
        // The marker contributes no intrinsic inline-size, so…
        self.block_flow.bubble_inline_sizes()
    }

    fn assign_inline_sizes(&mut self, layout_context: &LayoutContext) {
        self.block_flow.assign_inline_sizes(layout_context);
    }

    fn assign_block_size(&mut self, layout_context: &LayoutContext) {
        self.block_flow.assign_block_size(layout_context);
        self.assign_marker_inline_sizes(layout_context);
        self.assign_marker_block_sizes(layout_context);
    }

    fn compute_stacking_relative_position(&mut self, layout_context: &LayoutContext) {
        self.block_flow
            .compute_stacking_relative_position(layout_context)
    }

    fn place_float_if_applicable<'a>(&mut self) {
        self.block_flow.place_float_if_applicable()
    }

    fn contains_roots_of_absolute_flow_tree(&self) -> bool {
        self.block_flow.contains_roots_of_absolute_flow_tree()
    }

    fn is_absolute_containing_block(&self) -> bool {
        self.block_flow.is_absolute_containing_block()
    }

    fn update_late_computed_inline_position_if_necessary(&mut self, inline_position: Au) {
        self.block_flow
            .update_late_computed_inline_position_if_necessary(inline_position)
    }

    fn update_late_computed_block_position_if_necessary(&mut self, block_position: Au) {
        self.block_flow
            .update_late_computed_block_position_if_necessary(block_position)
    }

    fn build_display_list(&mut self, state: &mut DisplayListBuildState) {
        // Draw the marker, if applicable.
        for marker in &mut self.marker_fragments {
            let stacking_relative_border_box = self
                .block_flow
                .base
                .stacking_relative_border_box_for_display_list(marker);
            marker.build_display_list(
                state,
                stacking_relative_border_box,
                BorderPaintingMode::Separate,
                DisplayListSection::Content,
                self.block_flow.base.clip,
                None,
            );
        }

        // Draw the rest of the block.
        self.block_flow
            .build_display_list_for_block(state, BorderPaintingMode::Separate)
    }

    fn collect_stacking_contexts(&mut self, state: &mut StackingContextCollectionState) {
        self.block_flow.collect_stacking_contexts(state);
    }

    fn repair_style(&mut self, new_style: &crate::ServoArc<ComputedValues>) {
        self.block_flow.repair_style(new_style)
    }

    fn compute_overflow(&self) -> Overflow {
        let mut overflow = self.block_flow.compute_overflow();
        let flow_size = self
            .block_flow
            .base
            .position
            .size
            .to_physical(self.block_flow.base.writing_mode);
        let relative_containing_block_size = &self
            .block_flow
            .base
            .early_absolute_position_info
            .relative_containing_block_size;

        for fragment in &self.marker_fragments {
            overflow.union(&fragment.compute_overflow(&flow_size, &relative_containing_block_size))
        }
        overflow
    }

    fn generated_containing_block_size(&self, flow: OpaqueFlow) -> LogicalSize<Au> {
        self.block_flow.generated_containing_block_size(flow)
    }

    /// The 'position' property of this flow.
    fn positioning(&self) -> Position {
        self.block_flow.positioning()
    }

    fn iterate_through_fragment_border_boxes(
        &self,
        iterator: &mut dyn FragmentBorderBoxIterator,
        level: i32,
        stacking_context_position: &Point2D<Au>,
    ) {
        self.block_flow.iterate_through_fragment_border_boxes(
            iterator,
            level,
            stacking_context_position,
        );

        for marker in &self.marker_fragments {
            if iterator.should_process(marker) {
                iterator.process(
                    marker,
                    level,
                    &marker
                        .stacking_relative_border_box(
                            &self.block_flow.base.stacking_relative_position,
                            &self
                                .block_flow
                                .base
                                .early_absolute_position_info
                                .relative_containing_block_size,
                            self.block_flow
                                .base
                                .early_absolute_position_info
                                .relative_containing_block_mode,
                            CoordinateSystem::Own,
                        )
                        .translate(stacking_context_position.to_vector()),
                );
            }
        }
    }

    fn mutate_fragments(&mut self, mutator: &mut dyn FnMut(&mut Fragment)) {
        self.block_flow.mutate_fragments(mutator);

        for marker in &mut self.marker_fragments {
            (*mutator)(marker)
        }
    }
}

/// The kind of content that `list-style-type` results in.
pub enum ListStyleTypeContent {
    None,
    StaticText(char),
    GeneratedContent(Box<GeneratedContentInfo>),
}

impl ListStyleTypeContent {
    /// Returns the content to be used for the given value of the `list-style-type` property.
    pub fn from_list_style_type(list_style_type: ListStyleType) -> ListStyleTypeContent {
        // Just to keep things simple, use a nonbreaking space (Unicode 0xa0) to provide the marker
        // separation.
        match list_style_type {
            ListStyleType::None => ListStyleTypeContent::None,
            ListStyleType::Disc |
            ListStyleType::Circle |
            ListStyleType::Square |
            ListStyleType::DisclosureOpen |
            ListStyleType::DisclosureClosed => {
                let text = generated_content::static_representation(list_style_type);
                ListStyleTypeContent::StaticText(text)
            },
            _ => ListStyleTypeContent::GeneratedContent(Box::new(GeneratedContentInfo::ListItem)),
        }
    }
}
