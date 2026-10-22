import React from 'react';
export const Card = ({ title, children, ...rest }: { title: string; children?: React.ReactNode; [k: string]: unknown }) => (
  <article title={title} {...rest}>{children}</article>
);
export function List({ items }: { items: string[] }) {
  return <ul>{items.map((item, index) => <li key={item} data-index={index}>{item}</li>)}</ul>;
}
