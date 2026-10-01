import { Download, Heart, ListMusic, Pencil, Pin, Play, SquareArrowOutUpRight, Trash2 } from "lucide-react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { useAnchoredMenu } from "../hooks/useAnchoredMenu";
import { useMenuFocus } from "../hooks/useMenuFocus";

export type CollectionMenuState = {
  kind: "album" | "artist" | "playlist";
  id: string;
  name: string;
  x: number;
  y: number;
};

type CollectionMenuProps = {
  canEdit: boolean;
  menu: CollectionMenuState;
  onClose: () => void;
  onDelete: () => void;
  onDownload: () => void;
  onEnqueue: () => void;
  onOpen?: () => void;
  onPin: () => void;
  onPlay: () => void;
  onRename: () => void;
  onToggleStar: () => void;
  pinned: boolean;
  starred: boolean;
};

/// One action set for a collection, whether it was opened from a card, a
/// library row, or the "…" button on the collection's own page.
export function CollectionMenu({
  canEdit, menu, onClose, onDelete, onDownload, onEnqueue, onOpen, onPin, onPlay, onRename, onToggleStar, pinned, starred,
}: CollectionMenuProps) {
  const menuRef = useMenuFocus(onClose);
  const style = useAnchoredMenu(menuRef, menu.x, menu.y);
  const stop = (action: () => void) => (event: ReactMouseEvent) => { event.stopPropagation(); action(); onClose(); };
  const isPlaylist = menu.kind === "playlist";
  return (
    <div aria-label={menu.name} className="context-menu" ref={menuRef} role="menu" style={style}>
      <button onClick={stop(onPlay)} role="menuitem" type="button"><Play fill="currentColor" size={15} />Play</button>
      {menu.kind !== "artist" && <button onClick={stop(onEnqueue)} role="menuitem" type="button"><ListMusic size={16} />Add to queue</button>}
      {onOpen && <button onClick={stop(onOpen)} role="menuitem" type="button"><SquareArrowOutUpRight size={15} />Open</button>}
      <span className="context-menu__separator" />
      {menu.kind !== "playlist" && <button onClick={stop(onToggleStar)} role="menuitem" type="button"><Heart fill={starred ? "currentColor" : "none"} size={16} />{menu.kind === "artist" ? (starred ? "Unfollow" : "Follow") : (starred ? "Remove from Your Library" : "Add to Your Library")}</button>}
      {menu.kind !== "artist" && <button onClick={stop(onDownload)} role="menuitem" type="button"><Download size={16} />Download</button>}
      <button onClick={stop(onPin)} role="menuitem" type="button"><Pin size={15} />{pinned ? "Unpin" : `Pin ${menu.kind}`}</button>
      {isPlaylist && canEdit && <>
        <span className="context-menu__separator" />
        <button onClick={stop(onRename)} role="menuitem" type="button"><Pencil size={15} />Edit details</button>
        <button className="context-menu__danger" onClick={stop(onDelete)} role="menuitem" type="button"><Trash2 size={15} />Delete</button>
      </>}
    </div>
  );
}
