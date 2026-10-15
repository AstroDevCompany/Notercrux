import { useRef, useState } from 'react';
import { NodeViewContent, NodeViewWrapper, type NodeViewProps } from '@tiptap/react';
import { IconPicker } from '../components/EmojiPicker';

/** Callout whose icon is a button opening an icon and colour picker. */
export function CalloutView({ node, updateAttributes }: NodeViewProps) {
  const icon = (node.attrs.icon as string) || '💡';
  const tone = (node.attrs.tone as string) || 'default';
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <NodeViewWrapper className="callout" data-type="callout" data-tone={tone}>
      <button
        ref={anchor}
        className="callout-icon"
        contentEditable={false}
        aria-label="Change callout icon and color"
        aria-haspopup="dialog"
        onClick={() => setOpen(!open)}
      >
        {icon}
      </button>
      {open && (
        <IconPicker
          anchor={anchor}
          icon={icon}
          tone={tone}
          onIcon={(i) => updateAttributes({ icon: i })}
          onTone={(t) => updateAttributes({ tone: t === 'default' ? null : t })}
          onClose={() => setOpen(false)}
        />
      )}
      <NodeViewContent className="callout-body" />
    </NodeViewWrapper>
  );
}
