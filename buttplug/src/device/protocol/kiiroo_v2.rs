use super::{
  fleshlight_launch_helper::get_speed,
  ButtplugDeviceResultFuture,
  ButtplugProtocol,
  ButtplugProtocolCommandHandler,
  ButtplugProtocolCreator,
};
use crate::{
  core::{
    errors::ButtplugError,
    messages::{
      self,
      ButtplugDeviceCommandMessageUnion,
      FleshlightLaunchFW12Cmd,
      MessageAttributesMap,
    },
  },
  device::{
    configuration_manager::DeviceProtocolConfiguration,
    protocol::{generic_command_manager::GenericCommandManager, ButtplugProtocolProperties},
    DeviceImpl,
    DeviceWriteCmd,
    Endpoint,
  },
};
use async_mutex::Mutex;
use futures::future::BoxFuture;
use std::sync::{
  atomic::{AtomicU8, Ordering::SeqCst},
  Arc,
};

#[derive(ButtplugProtocol, ButtplugProtocolProperties)]
pub struct KiirooV2 {
  name: String,
  message_attributes: MessageAttributesMap,
  _manager: Arc<Mutex<GenericCommandManager>>,
  stop_commands: Vec<ButtplugDeviceCommandMessageUnion>,
  previous_position: Arc<AtomicU8>,
}

impl KiirooV2 {
  pub(super) fn new(name: &str, message_attributes: MessageAttributesMap) -> Self {
    let manager = GenericCommandManager::new(&message_attributes);

    Self {
      name: name.to_owned(),
      message_attributes,
      stop_commands: manager.get_stop_commands(),
      _manager: Arc::new(Mutex::new(manager)),
      previous_position: Arc::new(AtomicU8::new(0)),
    }
  }
}

impl ButtplugProtocolCreator for KiirooV2 {
  fn new_protocol(name: &str, attrs: MessageAttributesMap) -> Box<dyn ButtplugProtocol> {
    Box::new(Self::new(name, attrs))
  }

  fn try_create(
    device_impl: &dyn DeviceImpl,
    configuration: DeviceProtocolConfiguration,
  ) -> BoxFuture<'static, Result<Box<dyn ButtplugProtocol>, ButtplugError>> {
    let msg = DeviceWriteCmd::new(Endpoint::Firmware, vec![0x0u8], true);
    let info_fut = device_impl.write_value(msg);
    let device_name = device_impl.name().to_owned();
    let endpoints = device_impl.endpoints();
    Box::pin(async move {
      info_fut.await?;
      let (names, attrs) = configuration.get_attributes(&device_name, &endpoints).unwrap();
      let name = names.get("en-us").unwrap();
      Ok(Self::new_protocol(name, attrs))
    })
  }
}

impl ButtplugProtocolCommandHandler for KiirooV2 {
  fn handle_linear_cmd(
    &self,
    device: Arc<Box<dyn DeviceImpl>>,
    message: messages::LinearCmd,
  ) -> ButtplugDeviceResultFuture {
    let v = message.vectors[0].clone();
    // In the protocol, we know max speed is 99, so convert here. We have to
    // use AtomicU8 because there's no AtomicF64 yet.
    let previous_position = self.previous_position.load(SeqCst);
    let distance = (previous_position as f64 - (v.position * 99f64)).abs() / 99f64;
    let fl_cmd = FleshlightLaunchFW12Cmd::new(
      message.device_index,
      (v.position * 99f64) as u8,
      (get_speed(distance, v.duration) * 99f64) as u8,
    );
    self.handle_fleshlight_launch_fw12_cmd(device, fl_cmd)
  }

  fn handle_fleshlight_launch_fw12_cmd(
    &self,
    device: Arc<Box<dyn DeviceImpl>>,
    message: messages::FleshlightLaunchFW12Cmd,
  ) -> ButtplugDeviceResultFuture {
    let previous_position = self.previous_position.clone();
    let position = message.position;
    let msg = DeviceWriteCmd::new(
      Endpoint::Tx,
      [message.position, message.speed].to_vec(),
      false,
    );
    let fut = device.write_value(msg);
    Box::pin(async move {
      previous_position.store(position, SeqCst);
      fut.await?;
      Ok(messages::Ok::default().into())
    })
  }
}

#[cfg(test)]
mod test {
  use crate::{
    core::messages::{FleshlightLaunchFW12Cmd, LinearCmd, VectorSubcommand},
    device::{DeviceImplCommand, DeviceWriteCmd, Endpoint},
    test::{check_recv_value, new_bluetoothle_test_device},
    util::async_manager,
  };

  #[test]
  pub fn test_kiiroov2_fleshlight_fw12cmd() {
    async_manager::block_on(async move {
      let (device, test_device) = new_bluetoothle_test_device("Launch").await.unwrap();
      let command_receiver = test_device
        .get_endpoint_channel(&Endpoint::Tx)
        .unwrap()
        .receiver;
      device
        .parse_message(FleshlightLaunchFW12Cmd::new(0, 50, 50).into())
        .await
        .unwrap();
      check_recv_value(
        &command_receiver,
        DeviceImplCommand::Write(DeviceWriteCmd::new(Endpoint::Tx, vec![50, 50], false)),
      )
      .await;
    });
  }

  #[test]
  pub fn test_kiiroov2_linearcmd() {
    async_manager::block_on(async move {
      let (device, test_device) = new_bluetoothle_test_device("Launch").await.unwrap();
      let command_receiver = test_device
        .get_endpoint_channel(&Endpoint::Tx)
        .unwrap()
        .receiver;
      device
        .parse_message(LinearCmd::new(0, vec![VectorSubcommand::new(0, 500, 0.5)]).into())
        .await
        .unwrap();
      check_recv_value(
        &command_receiver,
        DeviceImplCommand::Write(DeviceWriteCmd::new(Endpoint::Tx, vec![49, 19], false)),
      )
      .await;
    });
  }
}
