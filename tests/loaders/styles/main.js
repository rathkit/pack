import styles, { card } from "./card.module.css";
import "./app.css";
console.log(Object.keys(styles).join(), /^[a-z2-7]{5}_card$/.test(card), styles.title === card + " " + styles.title.split(" ")[0] || styles.title.split(" ").includes(card));
