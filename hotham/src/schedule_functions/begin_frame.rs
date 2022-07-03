use openxr::ActiveActionSet;

use crate::{
    resources::{xr_context::XrContext, RenderContext, VulkanContext},
    VIEW_TYPE,
};

/// Begin a frame
/// Make sure to call this BEFORE beginning any renderpasses.
pub fn begin_frame(
    xr_context: &mut XrContext,
    vulkan_context: &VulkanContext,
    render_context: &mut RenderContext,
) {
    let active_action_set = ActiveActionSet::new(&xr_context.action_set);
    xr_context
        .session
        .sync_actions(&[active_action_set])
        .unwrap();

    // Wait for a frame to become available from the runtime, then get its index.
    xr_context.begin_frame().unwrap();

    let (view_state_flags, views) = xr_context
        .session
        .locate_views(
            VIEW_TYPE,
            xr_context.frame_state.predicted_display_time,
            &xr_context.reference_space,
        )
        .unwrap();
    xr_context.views = views;
    xr_context.view_state_flags = view_state_flags;

    // If we shouldn't render yet, we're done.
    if !xr_context.frame_state.should_render {
        return;
    }

    render_context.begin_frame(vulkan_context, xr_context.frame_index);
}

#[cfg(target_os = "windows")]
#[cfg(test)]
mod tests {
    use crate::resources::{RenderContext, XrContext};

    use super::begin_frame;

    #[test]

    pub fn test_begin_frame() {
        let (mut xr_context, vulkan_context) = XrContext::new().unwrap();
        let mut render_context = RenderContext::new(&vulkan_context, &xr_context).unwrap();
        xr_context.frame_index = 100;

        begin_frame(&mut xr_context, &vulkan_context, &mut render_context);
        assert_eq!(xr_context.frame_index, 0);
    }
}
