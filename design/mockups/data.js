// Mock data for the style mockups. Fictional people and accounts; every
// "password" here is made up.

window.MOCK = (() => {
  const groups = [
    { id: 'root', name: '个人密码库', depth: 0, icon: 'vault' },
    { id: 'fav', name: '常用', depth: 1 },
    { id: 'work', name: '工作', depth: 1 },
    { id: 'srv', name: '服务器', depth: 2, parent: 'work' },
    { id: 'fin', name: '金融', depth: 1 },
    { id: 'social', name: '社交', depth: 1 },
    { id: 'game', name: '游戏', depth: 1 },
    { id: 'bin', name: '回收站', depth: 1, icon: 'trash' },
  ];

  const e = (o) => ({
    tags: [], fields: [], files: [], notes: '', totp: false, expired: false,
    created: '2023-04-02 09:13', expires: '永不', ...o,
  });

  const entries = [
    e({
      id: 'github', title: 'GitHub', user: 'linchen-dev', pw: 'Vq7#mZ2!pL9x$Rt4',
      url: 'https://github.com/login', group: 'fav', modified: '2026-11-12 14:20',
      tags: ['开发', '两步验证'], totp: true,
      fields: [
        { k: '恢复邮箱', v: 'lin.chen@outlook.com' },
        { k: '恢复码', v: '4f2c-9a1e-77b0-c3d5', secret: true },
      ],
      files: [{ name: 'github-recovery-codes.txt', size: '1.2 KB' }],
      notes: '两步验证用手机上的验证器。恢复码另存了一份在附件里。',
    }),
    e({ id: 'jd', title: '京东', user: 'linchen_jd', pw: 'kT9@wq3Lm#2v', url: 'https://www.jd.com', group: 'fav', modified: '2026-06-03 21:47' }),
    e({ id: 'mail163', title: '网易邮箱', user: 'linchen@163.com', pw: 'n8&Xr2pQ!vC5', url: 'https://mail.163.com', group: 'fav', modified: '2025-12-30 10:05', totp: true }),
    e({
      id: 'router', title: '家里路由器', user: 'admin', pw: 'Ht4!Rm8@qz', url: 'http://192.168.31.1', group: 'fav', modified: '2025-08-17 19:30',
      notes: '访客网络单独设了密码，见自定义字段。',
      fields: [{ k: '访客网络密码', v: 'guest-2025-home', secret: true }],
    }),

    e({ id: 'outlook', title: '工作邮箱', user: 'lin.chen@acme-corp.cn', pw: 'Wp3$kL8#nM2q', url: 'https://outlook.office.com', group: 'work', modified: '2026-10-08 09:12', tags: ['工作'] }),
    e({
      id: 'vpn', title: '公司 VPN', user: 'lin.chen', pw: 'Acme@2026Q3!', url: 'https://vpn.acme-corp.cn', group: 'work', modified: '2026-08-02 08:40',
      expired: true, expires: '2026-10-31 00:00', notes: '每 90 天必须改一次密码。', tags: ['工作'],
    }),
    e({ id: 'gitlab', title: 'GitLab', user: 'lin.chen', pw: 'g7Lq!2Wx#9Pz', url: 'https://git.acme-corp.cn/users/sign_in', group: 'work', modified: '2026-09-21 16:02', totp: true, tags: ['工作', '开发'] }),
    e({ id: 'jenkins', title: 'Jenkins', user: 'linchen', pw: 'J3nk!ns-ci-88', url: 'https://ci.acme-corp.cn', group: 'work', modified: '2026-03-11 11:26' }),
    e({ id: 'wiki', title: 'Confluence', user: 'lin.chen@acme-corp.cn', pw: 'Cf8#mQ2!xL4p', url: 'https://wiki.acme-corp.cn', group: 'work', modified: '2026-01-19 14:55' }),
    e({
      id: 'aliyun', title: '阿里云', user: 'linchen@1849302217.onaliyun.com', pw: 'aY6!pR3#tW9m', url: 'https://signin.aliyun.com', group: 'work',
      modified: '2026-11-02 17:38', totp: true, tags: ['工作', '两步验证'],
    }),
    e({ id: 'notion', title: 'Notion', user: 'lin.chen@outlook.com', pw: 'No7!on-Lc-24', url: 'https://www.notion.so', group: 'work', modified: '2025-11-27 22:10' }),

    e({
      id: 'prod', title: '生产服务器', user: 'root', pw: 'Pr0d!x7Qz#4mK2', url: 'ssh://10.0.8.21', group: 'srv', modified: '2026-07-14 03:22', tags: ['服务器'],
      fields: [{ k: '端口', v: '22' }, { k: '私钥口令', v: 'id_ed25519-passphrase', secret: true }],
    }),
    e({ id: 'staging', title: '测试服务器', user: 'deploy', pw: 'St4g!ng-Dp-9', url: 'ssh://10.0.8.35', group: 'srv', modified: '2026-05-08 15:47', tags: ['服务器'] }),
    e({ id: 'pg', title: 'PostgreSQL 主库', user: 'app_rw', pw: 'pG#8wQ2!rT6x', url: 'postgres://db.internal:5432/orders', group: 'srv', modified: '2026-02-26 10:31' }),

    e({
      id: 'cmb', title: '招商银行', user: '林晨', pw: '851204', url: 'https://pbsz.ebank.cmbchina.com', group: 'fin', modified: '2024-09-06 12:00',
      fields: [{ k: '卡号尾号', v: '8821' }], notes: 'U 盾放在书桌左边的抽屉里。',
    }),
    e({ id: 'alipay', title: '支付宝', user: '138****2046', pw: 'Zf6#kW2!pQ8m', url: 'https://www.alipay.com', group: 'fin', modified: '2025-04-18 08:09' }),
    e({ id: 'tax', title: '个人所得税', user: '林晨', pw: 'Gs9!tX4#wL2q', url: 'https://etax.chinatax.gov.cn', group: 'fin', modified: '2026-03-30 20:15' }),

    e({ id: 'bili', title: 'Bilibili', user: '林晨不加班', pw: 'b1L!b1L-22x', url: 'https://www.bilibili.com', group: 'social', modified: '2025-10-01 23:41' }),
    e({ id: 'douban', title: '豆瓣', user: 'linchen', pw: 'Db7#nQ3!mK9w', url: 'https://www.douban.com', group: 'social', modified: '2024-12-12 13:13' }),

    e({ id: 'steam', title: 'Steam', user: 'linchen_gg', pw: 'St3am!gG-4x9', url: 'https://store.steampowered.com', group: 'game', modified: '2026-08-25 01:05', notes: 'Steam 令牌在手机上。', tags: ['游戏'] }),

    e({ id: 'oldbbs', title: '旧论坛账号', user: 'linchen2014', pw: 'oldpass2014', url: 'http://bbs.example.cn', group: 'bin', modified: '2019-05-20 18:00' }),
    e({ id: 'oldmail', title: '前公司邮箱', user: 'lin@oldcorp.cn', pw: 'Old#C0rp!x', url: 'https://mail.oldcorp.cn', group: 'bin', modified: '2022-06-30 17:59' }),
  ];

  const recent = [
    { name: '个人密码库.kdbx', path: 'D:\\同步盘\\个人密码库.kdbx', when: '今天 09:12' },
    { name: '工作.kdbx', path: 'D:\\同步盘\\工作.kdbx', when: '11 月 14 日' },
    { name: '家庭共享.kdbx', path: 'E:\\备份\\家庭共享.kdbx', when: '10 月 3 日' },
  ];

  return { groups, entries, recent, file: '个人密码库.kdbx' };
})();
