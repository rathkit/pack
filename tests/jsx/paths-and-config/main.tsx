import { render } from './render.js';
import { Greeting } from '@components/greeting';
import { shout, VERSION } from '@lib';
import { helper } from 'src/lib/helper';
console.log(render(<Greeting name={shout('ada')} />), VERSION, helper());
