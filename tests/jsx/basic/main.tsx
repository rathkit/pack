import { render } from './render.js';
import { Card, List } from './components.js';
import * as ui from './ui.js';
import React from 'react';

const name = 'World';
const items = ['a', 'b', 'c'];
const props = { id: 'spread', title: 'T' };
console.log(render(<div className="box" data-x="1" hidden>Hello, {name}!</div>));
console.log(render(<Card title="t" {...props} extra={1 + 1}>  child text  </Card>));
console.log(render(<List items={items} />));
console.log(render(<><span>one</span><span>two</span></>));
console.log(render(<ui.Button kind="primary" disabled={false}>go</ui.Button>));
console.log(render(
  <section>
    <h1>Title &amp; more &lt;stuff&gt; &#65;&#x42;</h1>
    {/* a comment */}
    <p>
      multi
      line   text
    </p>
    {items.map((i) => <b key={i}>{i}</b>)}
    {name === 'World' ? <i>yes</i> : null}
    <input value="it's" aria-label='say "hi"' xlink:href="#a" />
  </section>
));
const generic = <T,>(x: T): T => x;
console.log(generic(5), typeof React);
