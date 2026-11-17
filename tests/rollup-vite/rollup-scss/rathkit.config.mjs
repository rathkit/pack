import scss from 'rollup-plugin-scss';
import * as sass from 'sass';
export default { plugins: [scss({ sass, output: false, include: ["**/*.scss"] })] };
