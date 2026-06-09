// 备忘录用的两个自定义 TipTap 扩展：
//   LineHeight  —— 给段落/标题加 line-height（写进 HTML 内联样式，只读模式也生效、可云同步）
//   AnchorLink  —— 「两行互跳」：给两行打同一个 data-anchor-pair，点击其一跳到另一行
import { Extension } from "@tiptap/core";

const BLOCK_TYPES = ["paragraph", "heading"];

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    noteLineHeight: {
      /** 给选区内的段落/标题设置行高（传 null 清除） */
      setNoteLineHeight: (lineHeight: string | null) => ReturnType;
    };
    anchorLink: {
      /** 给选区所在块设置锚点配对 id */
      setBlockAnchor: (id: string) => ReturnType;
      /** 清除所有该 id 的锚点 */
      clearAnchorById: (id: string) => ReturnType;
    };
  }
}

export const LineHeight = Extension.create({
  name: "noteLineHeight",
  addGlobalAttributes() {
    return [
      {
        types: BLOCK_TYPES,
        attributes: {
          lineHeight: {
            default: null,
            parseHTML: (el) => (el as HTMLElement).style.lineHeight || null,
            renderHTML: (attrs) =>
              attrs.lineHeight ? { style: `line-height: ${attrs.lineHeight}` } : {},
          },
        },
      },
    ];
  },
  addCommands() {
    return {
      setNoteLineHeight:
        (lineHeight) =>
        ({ state, tr, dispatch }) => {
          const { from, to } = state.selection;
          let changed = false;
          state.doc.nodesBetween(from, to, (node, pos) => {
            if (BLOCK_TYPES.includes(node.type.name)) {
              tr.setNodeMarkup(pos, undefined, { ...node.attrs, lineHeight });
              changed = true;
            }
          });
          if (changed && dispatch) dispatch(tr);
          return changed;
        },
    };
  },
});

export const AnchorLink = Extension.create({
  name: "anchorLink",
  addGlobalAttributes() {
    return [
      {
        types: BLOCK_TYPES,
        attributes: {
          anchorPair: {
            default: null,
            parseHTML: (el) => (el as HTMLElement).getAttribute("data-anchor-pair"),
            renderHTML: (attrs) =>
              attrs.anchorPair
                ? { "data-anchor-pair": attrs.anchorPair, class: "anchor-line" }
                : {},
          },
        },
      },
    ];
  },
  addCommands() {
    return {
      setBlockAnchor:
        (id) =>
        ({ state, tr, dispatch }) => {
          const { from, to } = state.selection;
          let changed = false;
          state.doc.nodesBetween(from, to, (node, pos) => {
            if (BLOCK_TYPES.includes(node.type.name)) {
              tr.setNodeMarkup(pos, undefined, { ...node.attrs, anchorPair: id });
              changed = true;
            }
          });
          if (changed && dispatch) dispatch(tr);
          return changed;
        },
      clearAnchorById:
        (id) =>
        ({ state, tr, dispatch }) => {
          let changed = false;
          state.doc.descendants((node, pos) => {
            if (node.attrs && node.attrs.anchorPair === id) {
              tr.setNodeMarkup(pos, undefined, { ...node.attrs, anchorPair: null });
              changed = true;
            }
          });
          if (changed && dispatch) dispatch(tr);
          return changed;
        },
    };
  },
});
