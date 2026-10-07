import type { ComponentProps, ReactNode, Ref } from "react";
import { uiDirection } from "../../design/direction";
import { panel } from "../../design/tokens";
import { expandedSize, layerFade, partFade } from "./animations";
import { IslandLayer, IslandPart } from "./IslandLayer";
import type { TabId } from "./tabs";

// =============================================================================
// The open island's layout, in ONE place: the real ExpandedIsland, the tour's mock island and the
// gallery all render it. 400 x 440, radius 30 (the shape itself belongs to the shell), direction =
// the UI language. Every surface shares the panel inset (tokens.panel.inset = 12):
//
//   16 ┌ header row, 28 high: title (leading edge) .......... action (trailing edge)
//    8 ├ content, full panel width: the scroll area reserves a 12px gutter on both sides, so the
//      │ cards sit exactly 12 from the panel edge whether or not the list scrolls
//    8 ├ dock, 56 high
//   12 └
// =============================================================================

/** The tab title: the large title role at the leading edge of the header row. */
export const PANEL_TITLE_CLASS = "text-large-title text-fg truncate";

/**
 * The tab content box (an absolutely positioned child of the frame's body). Lists scroll inside
 * `island-scroll` (its own 12px gutters make the inset, see index.css); About holds still and
 * takes the inset as padding. The scroll box ends 8 short of the content's end so the last card
 * clears the bottom fade.
 */
export function panelBodyClass(tab: TabId): string {
  return tab === "about" ? "flex flex-col overflow-hidden px-panel-inset" : "flex flex-col overflow-y-auto island-scroll pb-2";
}

interface PanelFrameProps extends Omit<ComponentProps<typeof IslandLayer>, "name" | "fade" | "size" | "dir" | "children" | "title"> {
  /** The tab title (in the header row's leading cell). */
  title: ReactNode;
  /** The tab's header action (trailing cell), if it has one. */
  action?: ReactNode;
  /** The tab content: absolutely positioned boxes inside the body. */
  children: ReactNode;
  /** The dock. */
  dock: ReactNode;
  /** The body is a ref target (the tour scrolls it by code). */
  bodyRef?: Ref<HTMLDivElement>;
}

export function PanelFrame({ title, action, children, dock, bodyRef, className = "", style, ...rest }: PanelFrameProps) {
  return (
    <IslandLayer
      {...rest}
      name="expanded"
      fade={layerFade.expanded}
      size={expandedSize()}
      dir={uiDirection()}
      className={`island-expanded flex flex-col cursor-default text-fg ${className}`}
      style={{ ...style, paddingTop: panel.paddingTop, paddingBottom: panel.paddingBottom }}
    >
      <IslandPart anchor="top-start" fade={partFade.header} className="flex items-center flex-shrink-0 px-panel-inset" style={{ height: panel.headerHeight }}>
        <div className="grid justify-items-start min-w-0 flex-1">{title}</div>
        <div className="grid justify-items-end flex-shrink-0 ms-auto">{action}</div>
      </IslandPart>
      <IslandPart anchor="center" fade={partFade.body} className="flex-1 min-h-0 overflow-hidden w-full relative" style={{ marginTop: panel.headerGap }}>
        <div ref={bodyRef} className="absolute inset-0">
          {children}
        </div>
      </IslandPart>
      <IslandPart anchor="bottom" fade={partFade.dock} className="flex-shrink-0 px-panel-inset" style={{ marginTop: panel.dockGap }}>
        {dock}
      </IslandPart>
    </IslandLayer>
  );
}
