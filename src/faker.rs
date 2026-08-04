use fake::Fake;
use fake::faker::address::raw::*;
use fake::faker::boolean::raw::Boolean;
use fake::faker::company::raw::*;
use fake::faker::creditcard::raw::CreditCardNumber;
use fake::faker::currency::raw::*;
use fake::faker::filesystem::raw::*;
use fake::faker::http::raw::{RfcStatusCode, ValidStatusCode};
use fake::faker::internet::raw::*;
use fake::faker::lorem::raw::*;
use fake::faker::name::raw::*;
use fake::faker::number::raw::{Digit, NumberWithFormat};
use fake::faker::phone_number::raw::*;
use fake::locales::{
  AR_SA, CY_GB, DE_DE, EN, FR_FR, IT_IT, JA_JP, PT_BR, PT_PT, ZH_CN, ZH_TW,
};

/// Resolve a fake value for a key in the `fake.` namespace using the default
/// English locale.
///
/// Keys are dot-separated names like `name`, `email`, `city`, etc. The full
/// interpolation in a benchmark file looks like `{{ fake.name }}`.
pub fn resolve(key: &str) -> Option<String> {
  resolve_locale("en", key)
}

/// Generate a random RFC 4122 version 4 UUID formatted with hyphens.
///
/// The `fake` crate (4.x) does not ship a `Uuid` faker behind any of its
/// features, so this is generated directly with `rand` and `hex`.
fn random_uuid() -> String {
  let mut bytes = rand::random::<[u8; 16]>();

  // Set the version (4) and variant (RFC 4122) bits.
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  let encoded = hex::encode(bytes);
  format!(
    "{}-{}-{}-{}-{}",
    &encoded[0..8],
    &encoded[8..12],
    &encoded[12..16],
    &encoded[16..20],
    &encoded[20..32]
  )
}

macro_rules! resolve_with_locale {
  ($locale:expr, $key:expr) => {{
    match $key {
      // Name
      "name" => Some(Name($locale).fake::<String>()),
      "first_name" => Some(FirstName($locale).fake::<String>()),
      "last_name" => Some(LastName($locale).fake::<String>()),
      "title" => Some(Title($locale).fake::<String>()),
      "suffix" => Some(Suffix($locale).fake::<String>()),
      "name_with_title" => Some(NameWithTitle($locale).fake::<String>()),

      // Internet
      "email" => Some(SafeEmail($locale).fake::<String>()),
      "free_email" => Some(FreeEmail($locale).fake::<String>()),
      "username" => Some(Username($locale).fake::<String>()),
      "password" => Some(Password($locale, 8..20).fake::<String>()),
      "ipv4" => Some(IPv4($locale).fake::<String>()),
      "ipv6" => Some(IPv6($locale).fake::<String>()),
      "ip" => Some(IP($locale).fake::<String>()),
      "mac" => Some(MACAddress($locale).fake::<String>()),
      "user_agent" => Some(UserAgent($locale).fake::<String>()),
      "domain_suffix" => Some(DomainSuffix($locale).fake::<String>()),
      "free_email_provider" => Some(FreeEmailProvider($locale).fake::<String>()),
      "uuid" => Some(random_uuid()),

      // Phone
      "phone" => Some(PhoneNumber($locale).fake::<String>()),
      "cell" => Some(CellNumber($locale).fake::<String>()),

      // Lorem
      "word" => Some(Word($locale).fake::<String>()),
      "words" => Some(Words($locale, 3..8).fake::<Vec<String>>().join(" ")),
      "sentence" => Some(Sentence($locale, 3..8).fake::<String>()),
      "sentences" => Some(Sentences($locale, 3..6).fake::<Vec<String>>().join(" ")),
      "paragraph" => Some(Paragraph($locale, 3..6).fake::<String>()),
      "paragraphs" => Some(Paragraphs($locale, 2..4).fake::<Vec<String>>().join("\n\n")),

      // Address
      "city" => Some(CityName($locale).fake::<String>()),
      "city_prefix" => Some(CityPrefix($locale).fake::<String>()),
      "city_suffix" => Some(CitySuffix($locale).fake::<String>()),
      "country" => Some(CountryName($locale).fake::<String>()),
      "country_code" => Some(CountryCode($locale).fake::<String>()),
      "street" => Some(StreetName($locale).fake::<String>()),
      "street_suffix" => Some(StreetSuffix($locale).fake::<String>()),
      "state" => Some(StateName($locale).fake::<String>()),
      "state_abbr" => Some(StateAbbr($locale).fake::<String>()),
      "zip" => Some(ZipCode($locale).fake::<String>()),
      "postcode" => Some(PostCode($locale).fake::<String>()),
      "building_number" => Some(BuildingNumber($locale).fake::<String>()),
      "secondary_address" => Some(SecondaryAddress($locale).fake::<String>()),
      "time_zone" => Some(TimeZone($locale).fake::<String>()),
      "latitude" => Some(Latitude($locale).fake::<String>()),
      "longitude" => Some(Longitude($locale).fake::<String>()),

      // Company
      "company" => Some(CompanyName($locale).fake::<String>()),
      "company_suffix" => Some(CompanySuffix($locale).fake::<String>()),
      "catch_phrase" => Some(CatchPhrase($locale).fake::<String>()),
      "buzzword" => Some(Buzzword($locale).fake::<String>()),
      "buzzword_middle" => Some(BuzzwordMiddle($locale).fake::<String>()),
      "buzzword_tail" => Some(BuzzwordTail($locale).fake::<String>()),
      "bs" => Some(Bs($locale).fake::<String>()),
      "bs_verb" => Some(BsVerb($locale).fake::<String>()),
      "bs_adj" => Some(BsAdj($locale).fake::<String>()),
      "bs_noun" => Some(BsNoun($locale).fake::<String>()),
      "profession" => Some(Profession($locale).fake::<String>()),
      "industry" => Some(Industry($locale).fake::<String>()),

      // Currency / Credit card
      "currency_code" => Some(CurrencyCode($locale).fake::<String>()),
      "currency_name" => Some(CurrencyName($locale).fake::<String>()),
      "currency_symbol" => Some(CurrencySymbol($locale).fake::<String>()),
      "credit_card" => Some(CreditCardNumber($locale).fake::<String>()),

      // Filesystem
      "file_path" => Some(FilePath($locale).fake::<String>()),
      "file_name" => Some(FileName($locale).fake::<String>()),
      "file_extension" => Some(FileExtension($locale).fake::<String>()),
      "dir_path" => Some(DirPath($locale).fake::<String>()),

      // Number / Boolean
      "digit" => Some(Digit($locale).fake::<String>()),
      "boolean" => Some(Boolean($locale, 50).fake::<bool>().to_string()),
      "number" => Some(NumberWithFormat($locale, "##########").fake::<String>()),

      // HTTP
      "status_code" => Some(ValidStatusCode($locale).fake::<String>()),
      "rfc_status_code" => Some(RfcStatusCode($locale).fake::<String>()),

      _ => None,
    }
  }};
}

pub fn resolve_locale(locale: &str, key: &str) -> Option<String> {
  match locale {
    "en" => resolve_with_locale!(EN, key),
    "zh_cn" => resolve_with_locale!(ZH_CN, key),
    "zh_tw" => resolve_with_locale!(ZH_TW, key),
    "fr_fr" => resolve_with_locale!(FR_FR, key),
    "de_de" => resolve_with_locale!(DE_DE, key),
    "it_it" => resolve_with_locale!(IT_IT, key),
    "ja_jp" => resolve_with_locale!(JA_JP, key),
    "pt_br" => resolve_with_locale!(PT_BR, key),
    "pt_pt" => resolve_with_locale!(PT_PT, key),
    "ar_sa" => resolve_with_locale!(AR_SA, key),
    "cy_gb" => resolve_with_locale!(CY_GB, key),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolves_common_fakes() {
    assert!(!resolve("name").unwrap().is_empty());
    assert!(!resolve("email").unwrap().is_empty());
    assert!(resolve("email").unwrap().contains('@'));
    assert!(!resolve("city").unwrap().is_empty());
  }

  #[test]
  fn resolves_localized_fakes() {
    assert!(!resolve_locale("zh_cn", "name").unwrap().is_empty());
    assert!(!resolve_locale("fr_fr", "city").unwrap().is_empty());
    assert!(!resolve_locale("ja_jp", "name").unwrap().is_empty());
  }

  #[test]
  fn returns_none_for_unknown_locale() {
    assert!(resolve_locale("xx", "name").is_none());
  }

  #[test]
  fn returns_none_for_unknown_key() {
    assert!(resolve("not_a_real_faker").is_none());
  }
}
