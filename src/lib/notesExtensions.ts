// 备忘录用的两个自定义 TipTap 扩展：
//   LineHeight  —— 给段落/标题加 line-height（写进 HTML 内联样式，只读模式也生效、可云同步）
//   AnchorMark  —— 「两词互跳」：给两段选中文字打同一个 data-anchor-pair，点击其一跳到另一处
import { Extension, Mark, mergeAttributes } from "@tiptap/core";

const BLOCK_TYPES = ["paragraph", "heading"];

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    noteLineHeight: {
      /** 给选区内的段落/标题设置行高（传 null 清除） */
      setNoteLineHeight: (lineHeight: string | null) => ReturnType;
    };
    anchorMark: {
      /** 给当前选中文字打上锚点配对 id */
      setAnchorMark: (id: string) => ReturnType;
      /** 清除全文中所有该 id 的锚点 */
      clearAnchorMarkById: (id: string) => ReturnType;
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

export const AnchorMark = Mark.create({
  name: "anchorMark",
  inclusive: false, // 不在锚点末尾继续输入时自动延展
  addAttributes() {
    return {
      pairId: {
        default: null,
        parseHTML: (el) => (el as HTMLElement).getAttribute("data-anchor-pair"),
        renderHTML: (attrs) => (attrs.pairId ? { "data-anchor-pair": attrs.pairId } : {}),
      },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-anchor-pair]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: "anchor-link" }), 0];
  },
  addCommands() {
    return {
      setAnchorMark:
        (id) =>
        ({ commands }) =>
          commands.setMark(this.name, { pairId: id }),
      clearAnchorMarkById:
        (id) =>
        ({ state, tr, dispatch }) => {
          const markType = state.schema.marks[this.name];
          let changed = false;
          state.doc.descendants((node, pos) => {
            if (!node.isText) return;
            if (node.marks.some((m) => m.type === markType && m.attrs.pairId === id)) {
              tr.removeMark(pos, pos + node.nodeSize, markType);
              changed = true;
            }
          });
          if (changed && dispatch) dispatch(tr);
          return changed;
        },
    };
  },
});
