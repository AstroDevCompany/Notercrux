import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { Highlighted } from './CommandPalette';

describe('search highlighting', () => {
  it('turns backend match markers into <mark> elements and never into markup', () => {
    const { container } = render(
      <Highlighted text={'a \u0001lazy\u0002 <b>dog</b> \u0001x\u0002'} />,
    );
    const marks = [...container.querySelectorAll('mark')].map((m) => m.textContent);
    expect(marks).toEqual(['lazy', 'x']);
    expect(container.querySelector('b')).toBeNull(); // user text stays text
    expect(container.textContent).toBe('a lazy <b>dog</b> x');
  });

  it('tolerates an unterminated marker', () => {
    const { container } = render(<Highlighted text={'broken \u0001tail'} />);
    expect(container.textContent).toBe('broken tail');
  });
});
