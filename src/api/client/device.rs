use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{Err, err};
use futures::StreamExt;
use ruma::{
	MilliSecondsSinceUnixEpoch,
	api::client::{
		device::{self, delete_device, delete_devices, get_device, get_devices, update_device},
		error::ErrorKind,
		uiaa::{AuthFlow, AuthType, UiaaInfo},
	},
};

use super::SESSION_ID_LENGTH;
use crate::{Error, Result, Ruma, utils};

/// # `GET /_matrix/client/r0/devices`
///
/// Get metadata on all devices of the sender user.
pub(crate) async fn get_devices_route(
	State(services): State<crate::State>,
	body: Ruma<get_devices::v3::Request>,
) -> Result<get_devices::v3::Response> {
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");

	let devices: Vec<device::Device> = services
		.users
		.all_devices_metadata(sender_user)
		.collect()
		.await;

	Ok(get_devices::v3::Response { devices })
}

/// # `GET /_matrix/client/r0/devices/{deviceId}`
///
/// Get metadata on a single device of the sender user.
pub(crate) async fn get_device_route(
	State(services): State<crate::State>,
	body: Ruma<get_device::v3::Request>,
) -> Result<get_device::v3::Response> {
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");

	let device = services
		.users
		.get_device_metadata(sender_user, &body.body.device_id)
		.await
		.map_err(|_| err!(Request(NotFound("Device not found."))))?;

	Ok(get_device::v3::Response { device })
}

/// # `PUT /_matrix/client/r0/devices/{deviceId}`
///
/// Updates the metadata on a given device of the sender user.
#[tracing::instrument(skip_all, fields(%client), name = "update_device")]
pub(crate) async fn update_device_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<update_device::v3::Request>,
) -> Result<update_device::v3::Response> {
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");

	let mut device = services
		.users
		.get_device_metadata(sender_user, &body.device_id)
		.await
		.map_err(|_| err!(Request(NotFound("Device not found."))))?;

	device.display_name.clone_from(&body.display_name);
	device.last_seen_ip.clone_from(&Some(client.to_string()));
	device
		.last_seen_ts
		.clone_from(&Some(MilliSecondsSinceUnixEpoch::now()));

	services
		.users
		.update_device_metadata(sender_user, &body.device_id, &device)
		.await?;

	Ok(update_device::v3::Response {})
}

/// # `DELETE /_matrix/client/r0/devices/{deviceId}`
///
/// Deletes the given device.
///
/// - Requires UIAA to verify user password
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
pub(crate) async fn delete_device_route(
	State(services): State<crate::State>,
	body: Ruma<delete_device::v3::Request>,
) -> Result<delete_device::v3::Response> {
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");
	let sender_device = body.sender_device.as_ref().expect("user is authenticated");

	// UIAA
	let mut uiaainfo = UiaaInfo {
		flows: vec![AuthFlow { stages: vec![AuthType::Password] }],
		completed: Vec::new(),
		params: Box::default(),
		session: None,
		auth_error: None,
	};

	match &body.auth {
		| Some(auth) => {
			let (worked, uiaainfo) = services
				.uiaa
				.try_auth(sender_user, sender_device, auth, &uiaainfo)
				.await?;

			if !worked {
				return Err!(Uiaa(uiaainfo));
			}
			// Success!
		},
		| _ => match body.json_body {
			| Some(json) => {
				uiaainfo.session = Some(utils::random_string(SESSION_ID_LENGTH));
				services
					.uiaa
					.create(sender_user, sender_device, &uiaainfo, &json);

				return Err!(Uiaa(uiaainfo));
			},
			| _ => {
				return Err!(Request(NotJson("Not json.")));
			},
		},
	}

	services
		.users
		.remove_device(sender_user, &body.device_id)
		.await;

	Ok(delete_device::v3::Response {})
}

/// # `PUT /_matrix/client/r0/devices/{deviceId}`
///
/// Deletes the given device.
///
/// - Requires UIAA to verify user password
///
/// For each device:
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
pub(crate) async fn delete_devices_route(
	State(services): State<crate::State>,
	body: Ruma<delete_devices::v3::Request>,
) -> Result<delete_devices::v3::Response> {
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");
	let sender_device = body.sender_device.as_ref().expect("user is authenticated");

	// UIAA
	let mut uiaainfo = UiaaInfo {
		flows: vec![AuthFlow { stages: vec![AuthType::Password] }],
		completed: Vec::new(),
		params: Box::default(),
		session: None,
		auth_error: None,
	};

	match &body.auth {
		| Some(auth) => {
			let (worked, uiaainfo) = services
				.uiaa
				.try_auth(sender_user, sender_device, auth, &uiaainfo)
				.await?;

			if !worked {
				return Err(Error::Uiaa(uiaainfo));
			}
			// Success!
		},
		| _ => match body.json_body {
			| Some(json) => {
				uiaainfo.session = Some(utils::random_string(SESSION_ID_LENGTH));
				services
					.uiaa
					.create(sender_user, sender_device, &uiaainfo, &json);

				return Err(Error::Uiaa(uiaainfo));
			},
			| _ => {
				return Err(Error::BadRequest(ErrorKind::NotJson, "Not json."));
			},
		},
	}

	for device_id in &body.devices {
		services.users.remove_device(sender_user, device_id).await;
	}

	Ok(delete_devices::v3::Response {})
}
