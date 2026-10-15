# ADR 0027: Scrollbar

Status: accepted · 2026-10-14 (requested by the maintainer)

## Context

Users asked for a scrollbar: with only the wheel, getting to the bottom of a long list or a
long page is slow ("people want to pull down faster"). GPUI draws none. Zed's `Scrollbar` lives
in its `ui` crate, which is not a dependency we can take (ADR 0001 pins GPUI alone), and the
scrollbar helpers inside GPUI only serve its `list()` element, which we do not use.

## Decisions

1. **D1: our own element in `cloudrs-ui::scrollbar`.** One `scrollbar(id, handle, label)`
   component, no new dependency.
2. **D2: one `ScrollHandle`, no trait.** A `uniform_list`'s `UniformListScrollHandle` holds a
   public `base_handle: ScrollHandle`, and `track_scroll` registers exactly that one, so the same
   bar serves lists and plain scrolling divs. `list_scroll_handle` hides the field access. Moving
   that shared offset keeps virtualization: the list reads it in prepaint to pick its rows.
3. **D3: the geometry is measured in prepaint.** GPUI writes a handle's `max_offset` and
   `bounds` while it prepaints the content. The bar is therefore an element that comes after the
   content in the tree and measures there; measuring in `render` lags a frame and the bar would
   not show on first open, because nothing re-renders later.
4. **D4: always visible while there is overflow.** The OS "auto-hide scroll bars" setting is
   ignored on purpose: users asked to see it, and hiding it again needs a timer, which is idle
   CPU. So there is no fade and no animation. Without overflow nothing is drawn and no hitbox is
   inserted.
5. **D5: a click on the track jumps.** The thumb centres on the click and the drag continues
   from there. Paging by a viewport per click would need an auto-repeat timer.
6. **D6: wheel and keyboard are untouched.** The strip blocks the mouse except for the wheel,
   so scrolling over it still works. The bar is not focusable.
7. **D7: scope.** Every scrolling content area of the main window: the virtual lists, the
   pages, the sidebar playlists and the command palette. The add-to-playlist `menu()` has no
   bar, and the mini player scrolls nothing.

## Consequences

- The 12 px strip takes the pointer from the rightmost 12 px of the content while there is
  overflow; rows have 12 px of inner padding there, so nothing clickable is covered.
- Dragging moves the offset directly and notifies the view, like the wheel does.
- The five simple pages share one `ScrollHandle`, reset on navigation, so a page still opens at
  the top.
