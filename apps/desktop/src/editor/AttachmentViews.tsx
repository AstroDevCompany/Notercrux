import { useEffect, useState } from 'react';
import { NodeViewWrapper, type NodeViewProps } from '@tiptap/react';
import { api } from '../api';
import type { AttachmentMeta } from '../api/types';
import { actions } from '../state/app';
import { formatBytes } from '../state/sync';
import { Icon } from '../components/Icon';

interface Loaded {
  meta: AttachmentMeta | null;
  url: string | null;
  checked: boolean;
}

/**
 * Follows an attachment: waits while it downloads from another device, and (for images) turns the
 * bytes into an object URL for the `<img>`.
 */
function useAttachment(id: string | null, wantBytes: boolean): Loaded {
  const [state, setState] = useState<Loaded>({ meta: null, url: null, checked: false });
  useEffect(() => {
    if (!id) return setState({ meta: null, url: null, checked: true });
    let stop = false;
    let url: string | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const meta = await api.attachmentInfo(id);
        if (stop) return;
        const complete =
          !!meta && meta.chunksPresent >= meta.chunksTotal && meta.state !== 'failed';
        if (complete && wantBytes && !url) {
          const bytes = await api.readAttachment(id);
          if (stop) return;
          url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: meta.mime }));
        }
        setState({ meta, url, checked: true });
        // Not here yet (other device still uploading, or downloading now): look again shortly.
        if (!complete && meta?.state !== 'failed') timer = setTimeout(() => void poll(), 2500);
      } catch {
        if (!stop) {
          setState((s) => ({ ...s, checked: true }));
          timer = setTimeout(() => void poll(), 4000);
        }
      }
    };
    void poll();
    return () => {
      stop = true;
      clearTimeout(timer);
      if (url) URL.revokeObjectURL(url);
    };
  }, [id, wantBytes]);
  return state;
}

function Waiting({ meta, label }: { meta: AttachmentMeta | null; label: string }) {
  if (meta?.state === 'failed') {
    return (
      <div className="attachment-placeholder" role="alert">
        <Icon name="file" />
        <div className="meta">
          <div>{label} could not be downloaded.</div>
          <div className="setting-desc">{meta.error}</div>
        </div>
        <button className="btn" onClick={() => void api.syncRetryAttachments()}>
          Try again
        </button>
      </div>
    );
  }
  return (
    <div className="attachment-placeholder" role="status">
      <Icon name="download" />
      <div className="meta">
        {meta
          ? `Downloading ${label}…`
          : `${label} is not available yet. It appears once the device that added it has synced.`}
        {meta && meta.chunksTotal > 1 && (
          <div className="setting-desc">
            {meta.chunksPresent} of {meta.chunksTotal} parts
          </div>
        )}
      </div>
    </div>
  );
}

export function ImageView({ node, updateAttributes, selected }: NodeViewProps) {
  const id = (node.attrs.attachment as string | null) ?? null;
  const name = (node.attrs.name as string) || 'Image';
  const { meta, url } = useAttachment(id, true);
  return (
    <NodeViewWrapper
      className={`attachment attachment-image${selected ? ' is-selected' : ''}`}
      data-type="image"
      contentEditable={false}
    >
      {url ? (
        <>
          <img src={url} alt={(node.attrs.caption as string) || name} draggable={false} />
          <input
            className="attachment-caption"
            aria-label={`Caption for ${name}`}
            placeholder="Add a caption"
            value={(node.attrs.caption as string) ?? ''}
            onChange={(e) => updateAttributes({ caption: e.target.value })}
            onKeyDown={(e) => e.stopPropagation()}
          />
        </>
      ) : (
        <Waiting meta={meta} label={name} />
      )}
    </NodeViewWrapper>
  );
}

export function FileView({ node, selected }: NodeViewProps) {
  const id = (node.attrs.attachment as string | null) ?? null;
  const name = (node.attrs.name as string) || 'File';
  const size = Number(node.attrs.size) || 0;
  const { meta } = useAttachment(id, false);
  const ready = !!meta && meta.chunksPresent >= meta.chunksTotal && meta.state !== 'failed';
  return (
    <NodeViewWrapper
      className={`attachment${selected ? ' is-selected' : ''}`}
      data-type="file"
      contentEditable={false}
    >
      {ready || !meta ? (
        <div className="attachment-file">
          <Icon name="file" size={22} />
          <div className="meta">
            <div className="name">{name}</div>
            <div className="sub">
              {formatBytes(size)}
              {!meta && ' · not downloaded yet'}
            </div>
          </div>
          <button
            className="btn"
            disabled={!ready}
            onClick={() => {
              if (id)
                void api
                  .saveAttachmentAs(id)
                  .catch((e) => actions.notify('Could not save the file.', [String(e)]));
            }}
          >
            Save as…
          </button>
        </div>
      ) : (
        <Waiting meta={meta} label={name} />
      )}
    </NodeViewWrapper>
  );
}
