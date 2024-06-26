mod routine;
use log::{debug, info};
use once_cell::sync::Lazy;
use routine::*;

use chrono::{Duration, NaiveDateTime};
use rand::{thread_rng, Rng};
use reqwest::{header::*, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{digest::FixedOutputReset, Digest, Sha1};
use std::{collections::HashMap, error::Error};
use tokio::sync::{Mutex, RwLock};

const URL_CURRENT: &str = "https://cpes.legym.cn/education/semester/getCurrent";
const URL_GETRUNNINGLIMIT: &str = "https://cpes.legym.cn/running/app/getRunningLimit";
const URL_GETVERSION: &str =
    "https://cpes.legym.cn/authorization/mobileApp/getLastVersion?platform=2";
const URL_LOGIN: &str = "https://cpes.legym.cn/authorization/user/manage/login";
const URL_UPLOADRUNNING: &str = "https://cpes.legym.cn/running/app/v2/uploadRunningDetails";

const ORGANIZATION: HeaderName = HeaderName::from_static("organization");

const _HEADERS: [(HeaderName, &str); 9] = [
    (ACCEPT, "*/*"),
    (ACCEPT_ENCODING, "gzip, deflate, br"),
    (ACCEPT_LANGUAGE, "zh-CN, zh-Hans;q=0.9"),
    (AUTHORIZATION, ""),
    (CONNECTION, "keep-alive"),
    (CONTENT_TYPE, "application/json"),
    (HOST, "cpes.legym.cn"),
    (ORGANIZATION, ""),
    (USER_AGENT, "Mozilla/5.0 (iPhone; CPU iPhone OS 15_4_1 like Mac OSX) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Html15Plus/1.0 (Immersed/47) uni-app"),
];
static HEADERS: Lazy<RwLock<HeaderMap>> = Lazy::new(|| {
    let mut headers = HeaderMap::new();
    for (key, val) in _HEADERS {
        headers.insert(key, val.parse().unwrap());
    }
    RwLock::new(headers)
});
static HASHER: Lazy<Mutex<Sha1>> = Lazy::new(|| Mutex::new(Sha1::new()));

const CALORIE_PER_MILEAGE: f64 = 58.3;
const SALT: &str = "itauVfnexHiRigZ6";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Account {
    username: String,
    password: String,
    daily: f64,
    day: f64,
    end: f64,
    id: String,
    limitation: String,
    organization: String,
    scoring: i64,
    semester: String,
    start: f64,
    token: String,
    version: String,
    week: f64,
    weekly: f64,
}

impl Account {
    pub fn new(username: String, password: String) -> Self {
        Self::default().profile(username, password).to_owned()
    }

    pub fn profile(&mut self, username: String, password: String) -> &mut Self {
        self.username = username;
        self.password = password;
        self
    }

    #[must_use]
    pub async fn get_state(&mut self) -> Result<(), Box<dyn Error>> {
        self.get_current().await?;
        self.get_running_limit().await?;
        Ok(())
    }

    pub fn max_mileage(&self) -> f64 {
        (self.daily - self.day)
            .min(self.weekly - self.week)
            .min(self.end)
    }

    #[must_use]
    pub async fn login(&mut self) -> Result<(), Box<dyn Error>> {
        let client = Client::new();
        let signdigital = {
            let mut hasher = HASHER.lock().await;
            hasher.update((self.username.to_string() + &self.password + "1" + SALT).as_bytes());
            hex::encode(hasher.finalize_fixed_reset())
        };
        let json = json!({
            "entrance": "1",
            "password": &self.password.to_string(),
            "signDigital": &signdigital.to_string(),
            "userName": &self.username.to_string(),
        });

        debug!("Login json: {:#?}", json);

        let res = client
            .post(URL_LOGIN)
            .headers(HEADERS.read().await.clone())
            .json(&json)
            .send()
            .await?
            .error_for_status()?;

        #[derive(Deserialize, Debug)]
        #[allow(non_snake_case)]
        struct LoginData {
            id: String,
            accessToken: String,
            campusId: String,
        }

        #[derive(Deserialize)]
        struct LoginResult {
            data: LoginData,
        }

        debug!("Login response: {:#?}", res);
        let data = res
            .json::<LoginResult>()
            .await
            .or(Err("Login failed"))?
            .data;

        self.id = data.id;
        self.token = data.accessToken;
        self.organization = data.campusId;
        *HEADERS.write().await.get_mut(ORGANIZATION).unwrap() = self.organization.parse().unwrap();
        *HEADERS.write().await.get_mut(AUTHORIZATION).unwrap() =
            ("Bearer ".to_owned() + &self.token).parse().unwrap();

        self.get_version().await?;
        info!("Get token successful!");
        Ok(())
    }

    async fn get_current(&mut self) -> Result<(), Box<dyn Error>> {
        let client = Client::new();
        let res = client
            .get(URL_CURRENT)
            .headers(HEADERS.read().await.clone())
            .send()
            .await?
            .error_for_status()?;

        #[derive(Deserialize, Debug)]
        #[allow(non_snake_case)]
        struct CurrentData {
            id: String,
        }

        #[derive(Deserialize)]
        struct CurrentResult {
            data: CurrentData,
        }

        debug!("Current response: {:#?}", res);
        let data = res.json::<CurrentResult>().await?.data;

        self.semester = data.id;

        info!("Get current successful!");
        Ok(())
    }

    async fn get_version(&mut self) -> Result<(), Box<dyn Error>> {
        // Get Version
        let client = Client::new();
        let res = client
            .get(URL_GETVERSION)
            .headers(HEADERS.read().await.clone())
            .send()
            .await?
            .error_for_status()?;

        debug!("Version response: {:#?}", res);
        #[derive(Deserialize, Debug)]
        #[allow(non_snake_case)]
        struct VersionData {
            versionLabel: String,
        }

        #[derive(Deserialize)]
        struct VersionResult {
            data: VersionData,
        }
        let data = res.json::<VersionResult>().await?.data;

        self.version = data.versionLabel;

        info!("Get version successful!");
        Ok(())
    }

    async fn get_running_limit(&mut self) -> Result<(), Box<dyn Error>> {
        let client = Client::new();
        let json = json!({
            "semesterId": self.semester,
        });
        debug!("Running limits json: {:#?}", json);

        let res = client
            .post(URL_GETRUNNINGLIMIT)
            .headers(HEADERS.read().await.clone())
            .json(&json)
            .send()
            .await?
            .error_for_status()?;

        #[derive(Deserialize, Debug)]
        #[allow(non_snake_case)]
        struct RunningLimitsData {
            dailyMileage: Option<f64>,
            effectiveMileageEnd: Option<f64>,
            effectiveMileageStart: Option<f64>,
            limitationsGoalsSexInfoId: Option<String>,
            scoringType: Option<i64>,
            totalDayMileage: Option<String>,
            totalWeekMileage: Option<String>,
            weeklyMileage: Option<f64>,
        }

        #[derive(Deserialize)]
        struct RunningLimitsResult {
            data: RunningLimitsData,
        }

        debug!("Running limits response: {:#?}", res);
        let data = res.json::<RunningLimitsResult>().await?.data;

        if data.dailyMileage.is_none()
            || data.effectiveMileageEnd.is_none()
            || data.effectiveMileageStart.is_none()
            || data.limitationsGoalsSexInfoId.is_none()
            || data.scoringType.is_none()
            || data.totalDayMileage.is_none()
            || data.totalWeekMileage.is_none()
            || data.weeklyMileage.is_none()
        {
            return Err("Semester not started yet, Try again later.".into());
        }

        self.daily = data.dailyMileage.unwrap();
        self.day = data.totalDayMileage.unwrap().parse()?;
        self.end = data.effectiveMileageEnd.unwrap();
        self.limitation = data.limitationsGoalsSexInfoId.unwrap();
        self.scoring = data.scoringType.unwrap();
        self.start = data.effectiveMileageStart.unwrap();
        self.week = data.totalWeekMileage.unwrap().parse()?;
        self.weekly = data.weeklyMileage.unwrap();

        info!("Get running limitation successful!");
        Ok(())
    }

    pub async fn upload_running(
        &self,
        mut mileage: f64,
        datetime: NaiveDateTime,
        routefile: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let client = Client::new();
        let headers: HeaderMap<HeaderValue> = (&HashMap::from([
            (
                ACCEPT_ENCODING,
                "br;q=1.0, gzip;q=0.9, deflate;q=0.8".parse().unwrap(),
            ),
            (
                ACCEPT_LANGUAGE,
                "zh-Hans-HK;q=1.0, zh-Hant-HK;q=0.9, yue-Hant-HK;q=0.8"
                    .parse::<HeaderValue>()
                    .unwrap(),
            ),
            (
                AUTHORIZATION,
                ("Bearer ".to_owned() + &self.token).parse().unwrap(),
            ),
            (
                USER_AGENT,
                "QJGX/3.8.2 (com.ledreamer.legym; build:30000812; iOS 16.0.2) Alamofire/5.6.2"
                    .parse()
                    .unwrap(),
            ),
            (ACCEPT, "*/*".parse().unwrap()),
            (CONNECTION, "keep-alive".parse().unwrap()),
            (CONTENT_TYPE, "application/json".parse().unwrap()),
            (HOST, "cpes.legym.cn".parse().unwrap()),
        ]))
            .try_into()?;

        if mileage < self.start {
            return Err(format!(
                "Effective mileage too low, minimum is {}, but your input mileage is {}.",
                self.start, mileage
            )
            .into());
        }
        if mileage > self.end + 10. {
            return Err(format!(
                "Effective mileage too high, maximum is {}, but your input mileage is {}.",
                self.end, mileage
            )
            .into());
        }

        info!("Will running for {} miles...", mileage);

        let keeptime;
        let pace;
        {
            // WARN: Must make sure that the rng dies before the await call
            let mut rng = thread_rng();
            mileage += rng.gen_range(-0.02..-0.001);
            keeptime = (mileage * 1000.0 / 3.0) as i64 + rng.gen_range(-15..15);
            pace = 0.6 + rng.gen_range(-0.05..0.05);
        }

        let start_time = datetime - Duration::try_seconds(keeptime).unwrap();

        let signdigital = {
            let mut hasher = HASHER.lock().await;
            hasher.update(
                (mileage.to_string()
                    + "1"
                    + &start_time.format("%Y-%m-%d %H:%M:%S").to_string()
                    + &((CALORIE_PER_MILEAGE * mileage) as i64).to_string()
                    + &((keeptime as f64 / mileage) as i64 * 1000).to_string()
                    + &keeptime.to_string()
                    + &((mileage * 1000. / pace / 2.) as i64).to_string()
                    + &mileage.to_string()
                    + "1"
                    + SALT)
                    .as_bytes(),
            );
            hex::encode(hasher.finalize_fixed_reset())
        };
        let json = json!({
            "appVersion": self.version,
            "avePace": (keeptime as f64 / mileage) as i64 * 1000,
            "calorie": (CALORIE_PER_MILEAGE * mileage) as i64,
            "deviceType": "iPhone 13 Pro",
            "effectiveMileage": mileage,
            "effectivePart": 1,
            "endTime": datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
            "gpsMileage": mileage,
            "keepTime": keeptime,
            "limitationsGoalsSexInfoId": self.limitation,
            "paceNumber": (mileage * 1000. / pace / 2.) as i64,
            "paceRange": pace,
            "routineLine": get_routine(mileage, routefile)?,
            "scoringType": self.scoring,
            "semesterId": self.semester,
            "signDigital": signdigital,
            "signPoint": [],
            "startTime": start_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            "systemVersion": "16.0.2",
            "totalMileage": mileage,
            "totalPart": 1,
            "type": "范围跑",
            "uneffectiveReason": "",
        });

        debug!("Upload running json: {}", json.to_string());

        client
            .post(URL_UPLOADRUNNING)
            .headers(headers)
            .json(&json)
            .send()
            .await?
            .error_for_status()?;

        info!("Upload running successful!");
        Ok(())
    }
}
