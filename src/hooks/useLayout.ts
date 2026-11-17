// Pure helpers + hook for the tileable work-area layout tree (v0.5.0).
//
// Invariants kept by every operation below:
//  * every tab id appears in AT MOST one leaf (usually exactly one after
//    `ensureLayout` runs whenever the tab list changes);
//  * a split node never has a single child (it collapses into that child);
//  * a split node never directly contains another split of the same
//    direction (they get flattened — adjacent same-axis splits would make
//    gutter resizing ambiguous).
//
// The layout is intentionally in-memory only: splits are a session-time
// view concern, like the tab strip itself.

import { useCallback, useState } from "react";
import {
  DEFAULT_LAYOUT,
  type LayoutNode,
} from "../types";
import { uid } from "../lib/tree";

export type SplitDir = "row" | "col";

/** All leaf ids, left-to-right / top-to-bottom. */
export function collectLeaves(node: LayoutNode): Array<{ kind: "leaf"; id: string; tabId: string | null }> {
  if (node.kind === "leaf") return [node];
  return node.children.flatMap(collectLeaves);
}

/** The leaf currently showing `tabId`, if any. */
export function findLeafByTab(node: LayoutNode, tabId: string): string | null {
  if (node.kind === "leaf") return node.tabId === tabId ? node.id : null;
  for (const c of node.children) {
    const hit = findLeafByTab(c, tabId);
    if (hit) return hit;
  }
  return null;
}

function findLeaf(node: LayoutNode, leafId: string): LayoutNode | null {
  if (node.kind === "leaf") return node.id === leafId ? node : null;
  for (const c of node.children) {
    const hit = findLeaf(c, leafId);
    if (hit) return hit;
  }
  return null;
}

/** True if `leafId` exists in the tree. */
export function hasLeaf(node: LayoutNode, leafId: string): boolean {
  return findLeaf(node, leafId) !== null;
}

/** Replace the node at `leafId` via `fn` (identity when not found). */
function mapLeaf(
  node: LayoutNode,
  leafId: string,
  fn: (leaf: { kind: "leaf"; id: string; tabId: string | null }) => LayoutNode,
): LayoutNode {
  if (node.kind === "leaf") {
    return node.id === leafId ? fn(node) : node;
  }
  return { ...node, children: node.children.map((c) => mapLeaf(c, leafId, fn)) };
}

/** Collapse degenerate / nested splits and drop extra empty leaves. */
function normalize(node: LayoutNode): LayoutNode {
  if (node.kind === "leaf") return node;

  let children = node.children.map(normalize);

  // Flatten adjacent same-direction splits.
  children = children.flatMap((c) =>
    c.kind === "split" && c.dir === node.dir ? c.children : [c],
  );

  // Drop empty leaves, but keep one if the whole branch is empty.
  const nonEmpty = children.filter((c) => !(c.kind === "leaf" && c.tabId === null));
  if (nonEmpty.length > 0 && nonEmpty.length < children.length) {
    children = nonEmpty;
  } else if (nonEmpty.length === 0) {
    return { kind: "leaf", id: uid(), tabId: null };
  }

  // Sizes only make sense after filtering; recompute evenly when stale.
  if (children.length === 1) return children[0];

  const sizes =
    children.length === node.sizes.length
      ? node.sizes
      : children.map(() => 100 / children.length);

  return { ...node, children, sizes };
}

/**
 * Reconcile the tree with the tab list: prune leaves of closed tabs and
 * place newly opened tabs (first into empty leaves, then by splitting the
 * `preferredLeafId` leaf — usually the active one).
 */
export function ensureLayout(node: LayoutNode, tabIds: string[], preferredLeafId?: string | null): LayoutNode {
  const placed = new Set(collectLeaves(node).map((l) => l.tabId).filter((t): t is string => !!t));
  const alive = new Set(tabIds);

  // Prune dead tabs.
  let tree: LayoutNode = node.kind === "leaf"
    ? node.tabId && !alive.has(node.tabId)
      ? { ...node, tabId: null }
      : node
    : mapTree(node, (l) => (l.tabId && !alive.has(l.tabId) ? { ...l, tabId: null } : l));

  // Assign unplaced tabs.
  for (const tabId of tabIds) {
    if (placed.has(tabId)) continue;
    placed.add(tabId);
    tree = assignTab(tree, tabId, preferredLeafId);
  }

  return normalize(tree);
}

function mapTree(node: LayoutNode, fn: (l: { kind: "leaf"; id: string; tabId: string | null }) => LayoutNode): LayoutNode {
  if (node.kind === "leaf") return fn(node);
  return { ...node, children: node.children.map((c) => mapTree(c, fn)) };
}

function assignTab(node: LayoutNode, tabId: string, preferredLeafId?: string | null): LayoutNode {
  // 1. preferred leaf if it happens to be empty
  if (preferredLeafId) {
    const pref = findLeaf(node, preferredLeafId);
    if (pref && pref.kind === "leaf" && pref.tabId === null) {
      return mapLeaf(node, preferredLeafId, (l) => ({ ...l, tabId }));
    }
  }
  // 2. any empty leaf
  const empty = collectLeaves(node).find((l) => l.tabId === null);
  if (empty) return mapLeaf(node, empty.id, (l) => ({ ...l, tabId }));
  // 3. split the preferred leaf (or the first leaf)
  const targetId = preferredLeafId ?? collectLeaves(node)[0]?.id;
  if (!targetId) return { kind: "split", id: uid(), dir: "row", sizes: [50, 50], children: [node, { kind: "leaf", id: uid(), tabId }] };
  return splitLeaf(node, targetId, tabId, "row");
}

/** Split `leafId` along `dir`, moving its tab into the first pane. */
export function splitLeaf(node: LayoutNode, leafId: string, newTabId?: string | null, dir: SplitDir = "row"): LayoutNode {
  const out = mapLeaf(node, leafId, (l) => {
    const a: LayoutNode = { kind: "leaf", id: uid(), tabId: l.tabId };
    const b: LayoutNode = { kind: "leaf", id: uid(), tabId: newTabId ?? null };
    return { kind: "split", id: uid(), dir, sizes: [50, 50], children: [a, b] };
  });
  return normalize(out);
}

/**
 * Make `leafId` show `tabId`. If `tabId` currently lives in another leaf,
 * the two leaves swap their tabs (one tab per leaf stays true).
 */
export function moveTabToLeaf(node: LayoutNode, leafId: string, tabId: string): LayoutNode {
  const moved = findLeafByTab(node, tabId);
  const target = findLeaf(node, leafId);
  if (!target || target.kind !== "leaf") return node;
  if (moved === leafId) return node;

  const targetTab = (target as { tabId: string | null }).tabId;
  let out = mapLeaf(node, leafId, (l) => ({ ...l, tabId }));
  if (moved) out = mapLeaf(out, moved, (l) => ({ ...l, tabId: targetTab }));
  return normalize(out);
}

/** Leaf that becomes "next" when `leafId` is removed (for activation). */
export function neighborLeaf(node: LayoutNode, leafId: string): string | null {
  const leaves = collectLeaves(node);
  const idx = leaves.findIndex((l) => l.id === leafId);
  if (idx === -1) return null;
  const next = leaves[idx + 1] ?? leaves[idx - 1];
  return next && next.id !== leafId ? next.id : null;
}

/**
 * Remove `leafId`'s tab slot entirely (the tab itself is closed by the
 * caller via `closeTab`). Returns the pruned tree.
 */
export function closeLeaf(node: LayoutNode, leafId: string): LayoutNode {
  const out = mapLeaf(node, leafId, () => ({ kind: "leaf", id: leafId, tabId: null }));
  return normalize(out);
}

/** Resize a split's children (percent sizes; caller normalizes sum). */
export function resizeSplit(node: LayoutNode, splitId: string, sizes: number[]): LayoutNode {
  const fix = (n: LayoutNode): LayoutNode => {
    if (n.kind === "leaf") return n;
    if (n.id === splitId && n.children.length === sizes.length) {
      return { ...n, sizes };
    }
    return { ...n, children: n.children.map(fix) };
  };
  return fix(node);
}

/** Split whose id is `splitId` (for gutter math). */
export function findSplit(node: LayoutNode, splitId: string): LayoutNode | null {
  if (node.kind === "leaf") return null;
  if (node.id === splitId) return node;
  for (const c of node.children) {
    const hit = findSplit(c, splitId);
    if (hit) return hit;
  }
  return null;
}

export function useLayout() {
  const [layout, setLayout] = useState<LayoutNode>(DEFAULT_LAYOUT);

  const split = useCallback((leafId: string, dir: SplitDir) => {
    setLayout((t) => splitLeaf(t, leafId, null, dir));
  }, []);

  const move = useCallback((leafId: string, tabId: string) => {
    setLayout((t) => moveTabToLeaf(t, leafId, tabId));
  }, []);

  const close = useCallback((leafId: string) => {
    setLayout((t) => closeLeaf(t, leafId));
  }, []);

  const resize = useCallback((splitId: string, sizes: number[]) => {
    setLayout((t) => resizeSplit(t, splitId, sizes));
  }, []);

  const reconcile = useCallback((tabIds: string[], preferredLeafId?: string | null) => {
    setLayout((t) => ensureLayout(t, tabIds, preferredLeafId));
  }, []);

  return { layout, setLayout, split, move, close, resize, reconcile };
}
