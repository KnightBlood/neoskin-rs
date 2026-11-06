// NeoSkinRS 浏览器 E2E：设置 NEOSKIN_BASE_URL 后由 tests/browser.rs 调用。
// 依赖 playwright-core + 系统 Chrome（channel: chrome），不下载浏览器。
import assert from 'node:assert/strict'
import { chromium } from 'playwright-core'

const base = process.env.NEOSKIN_BASE_URL
if (!base) {
  throw new Error('NEOSKIN_BASE_URL is required')
}

const pageErrors = []
const browser = await chromium.launch({
  channel: process.env.NEOSKIN_CHROME_CHANNEL || 'chrome',
  executablePath: process.env.NEOSKIN_CHROME_PATH || undefined,
  headless: true,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})

try {
  const page = await browser.newPage({ locale: 'zh-CN' })
  page.on('console', (message) => {
    if (message.type() === 'error') {
      pageErrors.push(`console.error: ${message.text()}`)
    }
  })
  page.on('pageerror', (error) => {
    pageErrors.push(`pageerror: ${error.message}`)
  })

  // 1. 首页：SPA 壳注入 window.blessing。
  await page.goto(base + '/', { waitUntil: 'load' })
  await page.waitForSelector('body')
  const hasBlessing = await page.evaluate(
    () => typeof window.blessing === 'object' && window.blessing !== null,
  )
  assert.equal(hasBlessing, true, 'window.blessing missing on home page')

  // 2. 登录流程（React 表单）。
  await page.goto(base + '/auth/login', { waitUntil: 'load' })
  await page.waitForSelector('form input[type="password"]')
  await page.fill('form input[type="text"]', 'admin@example.com')
  await page.fill('form input[type="password"]', 'secret123')
  await page.click('form button')
  await page.waitForURL(/\/user$/, { timeout: 30000 })
  const userText = await page.evaluate(() => document.body.innerText)
  assert.ok(userText.includes('Admin'), 'user page missing nickname: ' + userText.slice(0, 200))

  // 3. 核心页面逐页检查关键元素。
  const pages = [
    { path: '/skinlib', selector: 'body' },
    { path: '/skinlib/upload', selector: '#previewer' },
    { path: '/user/profile', text: 'Admin' },
    { path: '/admin', text: 'Admin' },
    { path: '/admin/users', text: 'Admin' },
    { path: '/admin/communications', text: 'Admin' },
    { path: '/admin/update', text: '运行包更新' },
  ]
  for (const item of pages) {
    await page.goto(base + item.path, { waitUntil: 'load' })
    await page.waitForSelector('body')
    if (item.selector) {
      await page.waitForSelector(item.selector, { timeout: 15000 })
    }
    if (item.text) {
      const text = await page.evaluate(() => document.body.innerText)
      assert.ok(
        text.includes(item.text),
        `${item.path} missing ${JSON.stringify(item.text)}: ${text.slice(0, 200)}`,
      )
    }
  }

  // 4. 语言切换与前端契约。
  await page.goto(base + '/', { waitUntil: 'load' })
  const locale = await page.evaluate(() => window.blessing?.locale)
  assert.ok(locale, 'window.blessing.locale missing')

  // 5. 留出异步请求时间后统一断言 console error。
  await page.waitForTimeout(500)
  assert.deepEqual(pageErrors, [], 'browser console errors:\n' + pageErrors.join('\n'))

  console.log('browser E2E passed: home/login/skinlib/upload/user/admin/update')
} finally {
  await browser.close()
}
