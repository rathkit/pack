import Sitemap from 'vite-plugin-sitemap';
export default { plugins: [Sitemap({ hostname: 'https://x.test', dynamicRoutes: ['/a'], generateRobotsTxt: false })] };
