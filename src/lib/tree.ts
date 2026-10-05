// Pure, immutable helpers for the session tree. No React state here — these are
// shared by the `useSessionTree` hook and any future consumers (search, import preview).

import type { SessionNode } from "../types";

/** Return the node at `path` (array of child indices), or null. */
export function getAt(tree: SessionNode[], path: number[]): SessionNode | null {
  let nodes = tree;
  let node: SessionNode | null = null;
  for (const i of path) {
    node = nodes[i] ?? null;
    if (!node) return null;
    nodes = node.children;
  }
  return node;
}

/** Return a new tree with `patch` merged into the node at `path`. */
export function updateAt(
  tree: SessionNode[],
  path: number[],
  patch: Partial<SessionNode>,
): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  return tree.map((n, idx) => {
    if (idx !== i) return n;
    return rest.length === 0 ? { ...n, ...patch } : { ...n, children: updateAt(n.children, rest, patch) };
  });
}

/** Return a new tree with `node` inserted under `parentPath` (root when empty). */
export function insertAt(
  tree: SessionNode[],
  parentPath: number[],
  node: SessionNode,
): SessionNode[] {
  if (parentPath.length === 0) return [...tree, node];
  const [i, ...rest] = parentPath;
  return tree.map((n, idx) =>
    idx !== i ? n : { ...n, expanded: true, children: insertAt(n.children, rest, node) },
  );
}

/** Return a new tree with the node at `path` removed. */
export function removeAt(tree: SessionNode[], path: number[]): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  if (rest.length === 0) return tree.filter((_, idx) => idx !== i);
  return tree.map((n, idx) => (idx !== i ? n : { ...n, children: removeAt(n.children, rest) }));
}

/** Count connection (host) nodes in the tree. */
export function countConnections(tree: SessionNode[]): number {
  let n = 0;
  const walk = (nodes: SessionNode[]) => {
    for (const x of nodes) {
      if (x.type === "Connection") n += 1;
      if (x.children.length) walk(x.children);
    }
  };
  walk(tree);
  return n;
}

/** Map `fn` over every node, preserving structure. */
export function mapAll(
  tree: SessionNode[],
  fn: (n: SessionNode) => SessionNode,
): SessionNode[] {
  return tree.map((n) => fn({ ...n, children: n.children.length ? mapAll(n.children, fn) : [] }));
}

/**
 * Stable vault key for the node at `path`: the ancestor names joined by "/".
 * Used to associate a session's password with its vault entry.
 */
export function pathKey(tree: SessionNode[], path: number[]): string {
  const names: string[] = [];
  let nodes = tree;
  for (const i of path) {
    const n = nodes[i];
    if (!n) return "";
    names.push(n.name);
    nodes = n.children;
  }
  return names.join("/");
}

/** Return a new tree with connection passwords filled from a vault map (path key -> password). */
export function applyPasswords(
  tree: SessionNode[],
  map: Record<string, string>,
): SessionNode[] {
  const walk = (nodes: SessionNode[], prefix: string[]): SessionNode[] =>
    nodes.map((n) => {
      const path = [...prefix, n.name];
      const key = path.join("/");
      const next =
        n.type === "Connection" && map[key] ? { ...n, password: map[key] } : n;
      return { ...next, children: n.children.length ? walk(n.children, path) : [] };
    });
  return walk(tree, []);
}

/** Return a new tree with every connection password cleared (used on lock). */
export function clearPasswords(tree: SessionNode[]): SessionNode[] {
  return mapAll(tree, (n) => (n.type === "Connection" ? { ...n, password: "" } : n));
}

/** Generate a unique id (crypto.randomUUID when available). */
export function uid(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}