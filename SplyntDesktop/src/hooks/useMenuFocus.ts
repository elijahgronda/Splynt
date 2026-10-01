import { useEffect, useRef } from "react";

const menuItems = "[role='menuitem']:not([disabled])";

export function useMenuFocus<T extends HTMLElement = HTMLDivElement>(onClose: () => void) {
  const menuRef = useRef<T>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    const frame = window.requestAnimationFrame(() => menuRef.current?.querySelector<HTMLElement>(menuItems)?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      const menu = menuRef.current;
      if (!menu || !menu.contains(document.activeElement)) return;
      const items = Array.from(menu.querySelectorAll<HTMLElement>(menuItems));
      const current = items.indexOf(document.activeElement as HTMLElement);
      let next = current;
      if (event.key === "ArrowDown") next = current < 0 ? 0 : (current + 1) % items.length;
      else if (event.key === "ArrowUp") next = current < 0 ? items.length - 1 : (current - 1 + items.length) % items.length;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = items.length - 1;
      else if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      } else return;
      event.preventDefault();
      items[next]?.focus();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, []);
  return menuRef;
}
