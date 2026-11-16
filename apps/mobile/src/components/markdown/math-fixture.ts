// Captured from the real Tai BTW acceptance answer; no provider or authentication data.
export const TAI_P95_FORMULA = String.raw`p95 = \text{sorted}\big[\lceil 0.95\,n\rceil\big],\quad n=2 \Rightarrow \lceil 1.9\rceil = 2`;
export const TAI_MATH_ANSWER = `**算法不变：nearest-rank**

$$${TAI_P95_FORMULA}$$

取升序第 2 个元素，即**最大值**。不插值。

直接声明退化事实：\`n ≤ 19\` 时 $\\lceil 0.95n\\rceil = n$，p95 恒等于最大值。`;
