import React, { useState, createContext, useContext } from 'react';
import { renderToString } from 'react-dom/server';
import { Greeting } from './greeting.jsx';
import styles from './view.module.css';

const Theme = createContext('light');
function Counter({ start }) {
  const [n] = useState(start);
  const theme = useContext(Theme);
  return <p className={styles.counter} data-theme={theme}>count {n}</p>;
}
const html = renderToString(
  <Theme.Provider value="dark">
    <Greeting name="table" />
    <Counter start={3} />
    <>{[1, 2].map((i) => <i key={i}>{i}</i>)}</>
  </Theme.Provider>,
);
console.log(html.replace(/ class="[^"]*"/, ' class="css"'));
console.log(typeof React.version, Object.keys(styles).join(','));
