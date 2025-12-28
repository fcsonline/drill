use colored::*;

use crate::actions::Report;
use crate::reader;

pub fn compare(list_reports: &[Vec<Report>], filepath: &str, threshold: &str) -> Result<(), i32> {
  let threshold_value = match threshold.parse::<f64>() {
    Ok(v) => v,
    _ => panic!("arrrgh"),
  };

  let docs = reader::read_file_as_yml(filepath);
  let doc = &docs[0];
  let items = doc.as_sequence().unwrap();
  let mut slow_counter = 0;

  println!();

  for report in list_reports {
    for (i, report_item) in report.iter().enumerate() {
      let recorded_duration = items[i].get("duration").and_then(|v| v.as_f64()).unwrap();
      let delta_ms = report_item.duration - recorded_duration;

      if delta_ms > threshold_value {
        println!("{:width$} is {}{} slower than before", report_item.name.green(), delta_ms.round().to_string().red(), "ms".red(), width = 25);

        slow_counter += 1;
      }
    }
  }

  if slow_counter == 0 {
    Ok(())
  } else {
    Err(slow_counter)
  }
}
