import { render } from './render.js';
import React from 'react';

const Box = (p: { children?: React.ReactNode }) => <section>{p.children}</section>;
const pages = [1, 2, 3].map((n) => <b key={n}>{n}</b>);

// A spread child puts a list among the children, alone or with other children.
console.log(render(<Box>{...pages}</Box>));
console.log(render(<Box>before{...pages}after</Box>));
console.log(render(<ul>{...pages}</ul>));
console.log(render(<>{...pages}</>));
