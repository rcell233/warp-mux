use pathfinder_geometry::vector::Vector2F;
use warpui::elements::Point;
use warpui::event::DispatchedEvent;
use warpui::{
    AfterLayoutContext, AppContext, Element, Event, EventContext, LayoutContext, PaintContext,
    SizeConstraint,
};

use crate::workspace::WorkspaceAction;

pub(super) struct MuxFileDropTarget {
    child: Box<dyn Element>,
    origin: Option<Point>,
}

impl MuxFileDropTarget {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            origin: None,
        }
    }
}

impl Element for MuxFileDropTarget {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }
    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }
    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        self.child.paint(origin, ctx, app);
    }
    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }
    fn origin(&self) -> Option<Point> {
        self.origin
    }
    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let Some(origin) = self.origin {
            match event.raw_event() {
                Event::DragAndDropFiles { location, .. } | Event::DragFiles { location }
                    if ctx
                        .visible_rect(origin, self.size().unwrap())
                        .is_some_and(|rect| rect.contains_point(*location)) =>
                {
                    if let Event::DragAndDropFiles { paths, .. } = event.raw_event() {
                        ctx.dispatch_typed_action(WorkspaceAction::UploadMuxFiles(paths.clone()));
                    }
                    return true;
                }
                _ => {}
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}
