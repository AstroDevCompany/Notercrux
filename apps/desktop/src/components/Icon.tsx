import type { SVGProps } from 'react';

const PATHS = {
  chevron: 'M9 6l6 6-6 6',
  plus: 'M12 5v14M5 12h14',
  search: 'M11 4a7 7 0 105 12l4 4M11 4a7 7 0 010 14',
  sidebar: 'M4 5h16v14H4zM9 5v14',
  settings:
    'M12 15a3 3 0 100-6 3 3 0 000 6zM19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z',
  more: 'M5 12h.01M12 12h.01M19 12h.01',
  page: 'M7 3h7l5 5v13H7zM14 3v5h5',
  archive: 'M4 7h16v4H4zM6 11v9h12v-9M10 15h4',
  trash: 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
  copy: 'M9 9h11v11H9zM5 15V4h11',
  restore: 'M4 12a8 8 0 108-8M4 4v5h5',
  edit: 'M4 20h4L19 9l-4-4L4 16z',
  check: 'M5 12l5 5 9-10',
  table: 'M4 5h16v14H4zM4 10h16M4 15h16M10 5v14',
  back: 'M15 6l-6 6 6 6',
  forward: 'M9 6l6 6-6 6',
  filter: 'M4 5h16l-6 8v6l-4-2v-4z',
  sort: 'M8 5v14M5 16l3 3 3-3M16 19V5M13 8l3-3 3 3',
  x: 'M6 6l12 12M18 6L6 18',
  link: 'M10 14a4 4 0 005.7 0l3-3a4 4 0 00-5.7-5.7l-1 1M14 10a4 4 0 00-5.7 0l-3 3a4 4 0 005.7 5.7l1-1',
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  size = 16,
  ...rest
}: { name: IconName; size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
