import type { DOMOutputSpec, MarkSpec, NodeSpec } from 'prosemirror-model'
import { Schema } from 'prosemirror-model'
import { schema as basicSchema } from 'prosemirror-schema-basic'
import {
  addListNodes,
  bulletList,
  listItem,
  orderedList,
} from 'prosemirror-schema-list'
import { tableNodes } from 'prosemirror-tables'

const codeBlock: NodeSpec = {
  ...basicSchema.spec.nodes.get('code_block'),
  attrs: {
    language: { default: null, validate: 'string|null' },
  },
  parseDOM: [
    {
      tag: 'pre',
      preserveWhitespace: 'full',
      getAttrs(dom) {
        const element = dom as HTMLElement
        const code = element.querySelector('code')
        const className = code?.className ?? ''
        const languageClass = className
          .split(/\s+/)
          .find((name) => name.startsWith('language-'))
        return {
          language:
            element.dataset.language ?? languageClass?.slice(9) ?? null,
        }
      },
    },
  ],
  toDOM(node): DOMOutputSpec {
    const language = node.attrs.language as string | null
    const attributes = language ? { 'data-language': language } : {}
    const codeAttributes = language ? { class: `language-${language}` } : {}
    return ['pre', attributes, ['code', codeAttributes, 0]]
  },
}

const taskListItem: NodeSpec = {
  ...listItem,
  attrs: {
    checked: { default: null, validate: 'boolean|null' },
  },
  content: 'paragraph block*',
  parseDOM: [
    {
      tag: 'li',
      getAttrs(dom) {
        const value = (dom as HTMLElement).dataset.checked
        return {
          checked: value === undefined ? null : value === 'true',
        }
      },
    },
  ],
  toDOM(node): DOMOutputSpec {
    const checked = node.attrs.checked as boolean | null
    return checked === null
      ? ['li', 0]
      : [
          'li',
          {
            'data-checked': String(checked),
            'data-task-item': 'true',
          },
          0,
        ]
  },
}

const mathInline: NodeSpec = {
  atom: true,
  attrs: { formula: { default: '', validate: 'string' } },
  group: 'inline',
  inline: true,
  parseDOM: [
    {
      tag: 'span[data-dara-math="inline"]',
      getAttrs: (dom) => ({
        formula: (dom as HTMLElement).dataset.formula ?? '',
      }),
    },
  ],
  toDOM(node): DOMOutputSpec {
    const formula = node.attrs.formula as string
    return [
      'span',
      {
        'aria-label': `Math: ${formula}`,
        'data-dara-math': 'inline',
        'data-formula': formula,
        class: 'dara-math-node dara-math-inline',
      },
      formula,
    ]
  },
}

const mathDisplay: NodeSpec = {
  atom: true,
  attrs: { formula: { default: '', validate: 'string' } },
  group: 'block',
  parseDOM: [
    {
      tag: 'div[data-dara-math="display"]',
      getAttrs: (dom) => ({
        formula: (dom as HTMLElement).dataset.formula ?? '',
      }),
    },
  ],
  toDOM(node): DOMOutputSpec {
    const formula = node.attrs.formula as string
    return [
      'div',
      {
        'aria-label': `Display math: ${formula}`,
        'data-dara-math': 'display',
        'data-formula': formula,
        class: 'dara-math-node dara-math-display',
      },
      formula,
    ]
  },
}

const strike: MarkSpec = {
  parseDOM: [{ tag: 'del' }, { tag: 's' }, { tag: 'strike' }],
  toDOM: (): DOMOutputSpec => ['del', 0],
}

const daraImage: NodeSpec = {
  atom: true,
  attrs: {
    displayWidthPercent: { validate: 'number' },
    imageId: { validate: 'string' },
  },
  group: 'block',
  parseDOM: [
    {
      tag: 'figure[data-dara-image-id]',
      getAttrs(dom) {
        const element = dom as HTMLElement
        return {
          displayWidthPercent: Number(element.dataset.displayWidthPercent),
          imageId: element.dataset.daraImageId ?? '',
        }
      },
    },
  ],
  selectable: true,
  toDOM(node): DOMOutputSpec {
    return [
      'figure',
      {
        'data-dara-image-id': node.attrs.imageId,
        'data-display-width-percent': String(node.attrs.displayWidthPercent),
      },
    ]
  },
}

const pendingDaraImage: NodeSpec = {
  atom: true,
  attrs: { requestId: { validate: 'string' } },
  group: 'block',
  selectable: true,
  toDOM(): DOMOutputSpec {
    return ['div', { 'data-dara-image-pending': 'true' }, 'Processing image…']
  },
}

let nodes = basicSchema.spec.nodes.remove('image')
nodes = nodes.update('code_block', codeBlock)
nodes = addListNodes(nodes, 'paragraph block*', 'block')
nodes = nodes.update('ordered_list', {
  ...orderedList,
  content: 'list_item+',
  group: 'block',
})
nodes = nodes.update('bullet_list', {
  ...bulletList,
  content: 'list_item+',
  group: 'block',
})
nodes = nodes.update('list_item', taskListItem)
nodes = nodes.append(
  tableNodes({
    cellAttributes: {},
    cellContent: 'block+',
    tableGroup: 'block',
  }),
)
nodes = nodes.addBefore('text', 'math_inline', mathInline)
nodes = nodes.addBefore('text', 'math_display', mathDisplay)
nodes = nodes.addBefore('text', 'dara_image', daraImage)
nodes = nodes.addBefore('text', 'dara_image_pending', pendingDaraImage)

export const daraEditorSchema = new Schema({
  marks: basicSchema.spec.marks.append({ strike }),
  nodes,
})
