import { useState } from 'react';
import { NodeViewContent, NodeViewWrapper, type NodeViewProps } from '@tiptap/react';
import { CODE_LANGUAGES } from './languages';

/** Code block with a language selector and a copy button above the (editable) code. */
export function CodeBlockView({ node, updateAttributes }: NodeViewProps) {
  const language = (node.attrs.language as string | null) ?? '';
  const [copied, setCopied] = useState(false);
  // A language that is not in the list (e.g. imported from Markdown) is still shown and kept.
  const known = CODE_LANGUAGES.some((l) => l.id === language);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(node.textContent);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      /* clipboard unavailable: nothing to do */
    }
  };

  return (
    <NodeViewWrapper className="code-block">
      <div className="code-head" contentEditable={false}>
        <select
          className="code-lang"
          aria-label="Code language"
          value={language}
          onChange={(e) => updateAttributes({ language: e.target.value || null })}
        >
          {!known && <option value={language}>{language}</option>}
          {CODE_LANGUAGES.map((l) => (
            <option key={l.id} value={l.id}>
              {l.label}
            </option>
          ))}
        </select>
        <button className="code-copy" onClick={() => void copy()} aria-label="Copy code">
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre>
        <NodeViewContent<'code'>
          as="code"
          className={language ? `language-${language}` : undefined}
        />
      </pre>
    </NodeViewWrapper>
  );
}
