// Minecraft 官方语言列表：代码与原生名取自游戏语言文件；en_us 内置于客户端 jar，不在资源索引中。
// 原生名不参与界面翻译（语言名按其自身语言书写），i18n 门禁已排除本文件。

export interface MinecraftLanguage {
  code: string;
  name: string;
  region?: string;
}

/** 常用语言置顶，其余按代码排序；下拉按此顺序渲染。 */
export const MINECRAFT_LANGUAGES: MinecraftLanguage[] = [
  { code: "zh_cn", name: "简体中文", region: "中国大陆" },
  { code: "zh_tw", name: "繁體中文", region: "台灣" },
  { code: "zh_hk", name: "繁體中文", region: "香港特別行政區" },
  { code: "en_us", name: "English", region: "United States" },
  { code: "en_gb", name: "English", region: "United Kingdom" },
  { code: "en_au", name: "English", region: "Australia" },
  { code: "en_ca", name: "English", region: "Canada" },
  { code: "en_nz", name: "English", region: "New Zealand" },
  { code: "ja_jp", name: "日本語", region: "日本" },
  { code: "ko_kr", name: "한국어", region: "대한민국" },
  { code: "de_de", name: "Deutsch", region: "Deutschland" },
  { code: "fr_fr", name: "Français", region: "France" },
  { code: "es_es", name: "Español", region: "España" },
  { code: "ru_ru", name: "Русский", region: "Россия" },
  { code: "pt_br", name: "Português", region: "Brasil" },
  { code: "pt_pt", name: "Português", region: "Portugal" },
  { code: "it_it", name: "Italiano", region: "Italia" },
  { code: "pl_pl", name: "Polski", region: "Polska" },
  { code: "tr_tr", name: "Türkçe", region: "Türkiye" },
  { code: "uk_ua", name: "Українська", region: "Україна" },
  { code: "nl_nl", name: "Nederlands", region: "Nederland" },
  { code: "cs_cz", name: "Čeština", region: "Česko" },
  { code: "sv_se", name: "Svenska", region: "Sverige" },
  { code: "hu_hu", name: "Magyar", region: "Magyarország" },
  { code: "ro_ro", name: "Română", region: "România" },
  { code: "th_th", name: "ไทย", region: "ประเทศไทย" },
  { code: "vi_vn", name: "Tiếng Việt", region: "Việt Nam" },
  { code: "id_id", name: "Bahasa Indonesia", region: "Indonesia" },
  { code: "ms_my", name: "Bahasa Melayu", region: "Malaysia" },
  { code: "hi_in", name: "हिंदी", region: "भारत" },
  { code: "ar_sa", name: "العربية", region: "العالم العربي" },
  { code: "fa_ir", name: "فارسی", region: "ايران" },
  { code: "he_il", name: "עברית", region: "ישראל" },
  { code: "af_za", name: "Afrikaans", region: "Suid-Afrika" },
  { code: "ast_es", name: "Asturianu", region: "Asturies" },
  { code: "az_az", name: "Azərbaycanca", region: "Azərbaycan" },
  { code: "ba_ru", name: "Башҡортса", region: "Башҡортостан, Рәсәй" },
  { code: "bar", name: "Boarisch", region: "Bayern" },
  { code: "be_by", name: "Беларуская", region: "Беларусь" },
  { code: "be_latn", name: "Biełaruskaja", region: "Biełaruś" },
  { code: "bg_bg", name: "Български", region: "България" },
  { code: "br_fr", name: "Brezhoneg", region: "Breizh" },
  { code: "brb", name: "Braobans", region: "Braobant" },
  { code: "bs_ba", name: "Bosanski", region: "Bosna i Hercegovina" },
  { code: "ca_es", name: "Català", region: "Catalunya" },
  { code: "cbk_zam", name: "Chavacano de Zamboanga", region: "Filipinas" },
  { code: "cv_cu", name: "Чӑвашла", region: "Чӑваш Ен, Раҫҫей" },
  { code: "cy_gb", name: "Cymraeg", region: "Cymru" },
  { code: "da_dk", name: "Dansk", region: "Danmark" },
  { code: "de_at", name: "Deitsch", region: "Österreich" },
  { code: "de_ch", name: "Schwiizerdütsch", region: "Schwiiz" },
  { code: "el_gr", name: "Ελληνικά", region: "Ελλάδα" },
  { code: "en_pt", name: "Pirate Speak", region: "The Seven Seas" },
  { code: "en_ud", name: "ɥsᴉꞁᵷuƎ", region: "uʍoᗡ ǝpᴉsd∩" },
  { code: "enp", name: "Anglish", region: "Oned Riches" },
  { code: "enws", name: "Shakespearean English", region: "Kingdom of England" },
  { code: "eo_uy", name: "Esperanto", region: "Esperantujo" },
  { code: "es_ar", name: "Español", region: "Argentina" },
  { code: "es_cl", name: "Español", region: "Chile" },
  { code: "es_ec", name: "Español", region: "Ecuador" },
  { code: "es_mx", name: "Español", region: "México" },
  { code: "es_uy", name: "Español", region: "Uruguay" },
  { code: "es_ve", name: "Español", region: "Venezuela" },
  { code: "esan", name: "Andalûh", region: "Andaluçía" },
  { code: "et_ee", name: "Eesti keel", region: "Eesti" },
  { code: "eu_es", name: "Euskara", region: "Euskal Herria" },
  { code: "fi_fi", name: "Suomi", region: "Suomi" },
  { code: "fil_ph", name: "Filipino", region: "Pilipinas" },
  { code: "fo_fo", name: "Føroyskt", region: "Føroyar" },
  { code: "fr_ca", name: "Français", region: "Canada" },
  { code: "fr_ch", name: "Français", region: "Suisse" },
  { code: "fra_de", name: "Fränggisch", region: "Franggn" },
  { code: "fur_it", name: "Furlan", region: "Friûl" },
  { code: "fy_nl", name: "Frysk", region: "Fryslân" },
  { code: "ga_ie", name: "Gaeilge", region: "Éire" },
  { code: "gd_gb", name: "Gàidhlig", region: "Alba" },
  { code: "gl_es", name: "Galego", region: "Galicia / Galiza" },
  { code: "go_fr", name: "Galo", region: "Bertègn" },
  { code: "got_de", name: "𐌲𐌿𐍄𐍂𐌰𐌶𐌳𐌰", region: "𐌲𐌿𐍄𐌸𐌹𐌿𐌳𐌰" },
  { code: "hal_ua", name: "Галицка", region: "Галичина, Вкраїна" },
  { code: "haw_us", name: "ʻŌlelo Hawaiʻi", region: "Hawaiʻi" },
  { code: "hn_no", name: "Høgnorsk", region: "Norig" },
  { code: "hr_hr", name: "Hrvatski", region: "Hrvatska" },
  { code: "hy_am", name: "Հայերեն", region: "Հայաստան" },
  { code: "ig_ng", name: "Igbo", region: "Naigeria" },
  { code: "io_en", name: "Ido", region: "Idia" },
  { code: "is_is", name: "Íslenska", region: "Ísland" },
  { code: "isv", name: "Medžuslovjansky", region: "Slovjanščina" },
  { code: "jbo_en", name: "la .lojban.", region: "la jbogu'e" },
  { code: "jv_id", name: "Basa Jawa", region: "Jawa, Indhonésiyah" },
  { code: "ka_ge", name: "ქართული", region: "საქართველო" },
  { code: "kk_kz", name: "Қазақша", region: "Қазақстан" },
  { code: "kn_in", name: "ಕನ್ನಡ", region: "ಭಾರತ" },
  { code: "ksh", name: "Kölsch/Ripoarisch", region: "Rhingland" },
  { code: "kw_gb", name: "Kernewek", region: "Kernow" },
  { code: "ky_kg", name: "Кыргызча", region: "Кыргызстан" },
  { code: "la_la", name: "Lingua Latina", region: "Latium" },
  { code: "lb_lu", name: "Lëtzebuergesch", region: "Lëtzebuerg" },
  { code: "li_li", name: "Limburgs", region: "Limburg" },
  { code: "lmo", name: "Lombard", region: "Lombardia" },
  { code: "lo_la", name: "ລາວ", region: "ປະເທດລາວ" },
  { code: "lol_us", name: "LOLCAT", region: "Kingdom of Cats" },
  { code: "lt_lt", name: "Lietuvių", region: "Lietuva" },
  { code: "lv_lv", name: "Latviešu", region: "Latvija" },
  { code: "lzh", name: "文言", region: "華夏" },
  { code: "mk_mk", name: "Македонски", region: "Северна Македонија" },
  { code: "mn_mn", name: "Монгол", region: "Монгол Улс" },
  { code: "mt_mt", name: "Malti", region: "Malta" },
  { code: "nah", name: "Mēxikatlahtōlli", region: "Mēxiko" },
  { code: "nds_de", name: "Plattdüütsch", region: "Düütschland" },
  { code: "nl_be", name: "Vlaams", region: "België" },
  { code: "nn_no", name: "Norsk nynorsk", region: "Noreg" },
  { code: "no_no", name: "Norsk bokmål", region: "Norge" },
  { code: "oc_fr", name: "Occitan", region: "Occitània" },
  { code: "ovd", name: "Övdalska", region: "Swerre" },
  { code: "pls", name: "Ngiiwà", region: "Ndanìꞌngà" },
  { code: "qcb_es", name: "Cántabru/Montañés", region: "Cantabria" },
  { code: "qid", name: "Bhs. Indonesia edjaän lama", region: "Indonesia tempo doeloe" },
  { code: "qya_aa", name: "Quenya", region: "Arda" },
  { code: "rpr", name: "Русскій дореформенный", region: "Россійская имперія" },
  { code: "ry_ua", name: "Руснацькый", region: "Пудкарпатя, Украина" },
  { code: "sah_sah", name: "Сахалыы", region: "Cаха Сирэ" },
  { code: "se_no", name: "Davvisámegiella", region: "Sápmi" },
  { code: "sk_sk", name: "Slovenčina", region: "Slovensko" },
  { code: "sl_si", name: "Slovenščina", region: "Slovenija" },
  { code: "so_so", name: "Soomaali", region: "Soomaaliya" },
  { code: "sq_al", name: "Shqip", region: "Shqipëri" },
  { code: "sr_cs", name: "Srpski", region: "Srbija" },
  { code: "sr_sp", name: "Српски", region: "Србија" },
  { code: "sxu", name: "Säggs’sch", region: "Saggsn" },
  { code: "szl", name: "Ślōnski", region: "Gōrny Ślōnsk" },
  { code: "ta_in", name: "தமிழ்", region: "இந்தியா" },
  { code: "tl_ph", name: "Tagalog", region: "Pilipinas" },
  { code: "tlh_aa", name: "tlhIngan Hol", region: "tlhIngan wo'" },
  { code: "tok", name: "toki pona", region: "kulupu pona" },
  { code: "tt_ru", name: "Татарча", region: "Татарстан, Рәсәй" },
  { code: "tzo_mx", name: "Bats'i k'op", region: "Jobel" },
  { code: "uz_uz", name: "Özbekça", region: "Özbekiston" },
  { code: "val_es", name: "Català (Valencià)", region: "País Valencià" },
  { code: "vec_it", name: "Vèneto", region: "Veneto" },
  { code: "vp_vl", name: "Viossa", region: "Vilant" },
  { code: "vro", name: "Võro", region: "Eesti" },
  { code: "yi_de", name: "ייִדיש", region: "אשכנזיש יידן" },
  { code: "yo_ng", name: "Yorùbá", region: "Nàìjíríà" },
  { code: "zlm_arab", name: "بهاس ملايو", region: "مليسيا" },
];

const LANGUAGE_CODES = new Set(MINECRAFT_LANGUAGES.map((l) => l.code));

/** 系统语言子标签在 Minecraft 无精确地区变体时的首选代码。 */
const LANGUAGE_PREFERENCE: Record<string, string> = {
  ar: "ar_sa",
  de: "de_de",
  en: "en_us",
  es: "es_es",
  fr: "fr_fr",
  in: "id_id",
  iw: "he_il",
  ja: "ja_jp",
  ji: "yi_de",
  ko: "ko_kr",
  nb: "no_no",
  nl: "nl_nl",
  no: "no_no",
  pl: "pl_pl",
  pt: "pt_br",
  ru: "ru_ru",
  sr: "sr_sp",
  tr: "tr_tr",
  uk: "uk_ua",
  zh: "zh_cn",
};

/** 语言代码 → 下拉显示文本（原生名 + 原生地区名）。 */
export function minecraftLanguageLabel(code: string): string {
  const lang = MINECRAFT_LANGUAGES.find((l) => l.code === code);
  if (!lang) return code;
  return lang.region ? `${lang.name} (${lang.region})` : lang.name;
}

/** 把 BCP-47 标签（zh-CN / en-GB / es-419）映射到 Minecraft 语言代码；无法匹配返回 null。 */
export function resolveMinecraftLanguage(tag: string): string | null {
  const normalized = tag.trim().toLowerCase().replace(/-/g, "_");
  if (!normalized) return null;
  if (normalized === "es_419") return "es_mx";

  const parts = normalized.split("_");
  const lang = parts[0];
  const script = parts.find((p) => ["hans", "hant", "latn", "cyrl"].includes(p));
  const region = parts.slice(1).find((p) => /^[a-z]{2}$/.test(p)) ?? "";
  if (lang === "zh") {
    if (script === "hant" || ["tw", "hk", "mo"].includes(region)) {
      return region === "hk" || region === "mo" ? "zh_hk" : "zh_tw";
    }
    return "zh_cn";
  }
  if (lang === "sr") {
    if (script === "latn") return "sr_cs";
    if (script === "cyrl") return "sr_sp";
  }
  if (LANGUAGE_CODES.has(normalized)) return normalized;
  if (region && LANGUAGE_CODES.has(`${lang}_${region}`)) return `${lang}_${region}`;
  if (LANGUAGE_CODES.has(lang)) return lang;
  const preferred = LANGUAGE_PREFERENCE[lang];
  if (preferred) return preferred;
  return MINECRAFT_LANGUAGES.find((l) => l.code.startsWith(`${lang}_`))?.code ?? null;
}

/** 当前系统语言对应的 MC 语言；按浏览器语言优先级依次尝试，全部无法匹配时回退英文。 */
export function detectSystemLanguage(): string {
  const tags = typeof navigator === "undefined"
    ? []
    : (navigator.languages?.length ? navigator.languages : [navigator.language]);
  for (const tag of tags) {
    const code = resolveMinecraftLanguage(tag);
    if (code) return code;
  }
  return "en_us";
}

/** 配置值规范化：旧值 auto_zh_CN / auto 迁移为检测到的系统语言，其它非法值同样回退。 */
export function normalizeGameLanguage(value: string | null | undefined): string {
  const fallback = detectSystemLanguage();
  const code = value?.trim().replace(/-/g, "_").toLowerCase() ?? "";
  if (!code || code === "auto_zh_cn" || code === "auto") return fallback;
  return /^[a-z0-9_]+$/.test(code) ? code : fallback;
}
