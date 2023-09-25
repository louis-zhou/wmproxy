use std::{
    fs::File,
    io::{self, BufReader},
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use commander::Commander;
use rustls::{Certificate, PrivateKey};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::mpsc::{Receiver, Sender},
};
use tokio_rustls::{rustls, TlsAcceptor, TlsConnector};
use webparse::BinaryMut;

use crate::{
    error::ProxyTypeResult,
    prot::{ProtFrame, TransStream},
    Flag, ProxyError, ProxyHttp, ProxyResult, ProxySocks5, CenterClient, CenterServer,
};

pub struct Builder {
    inner: ProxyResult<ProxyOption>,
}

impl Builder {
    #[inline]
    pub fn new() -> Builder {
        Builder {
            inner: Ok(ProxyOption::default()),
        }
    }

    pub fn flag(self, flag: Flag) -> Builder {
        self.and_then(|mut proxy| {
            proxy.flag = flag;
            Ok(proxy)
        })
    }

    pub fn add_flag(self, flag: Flag) -> Builder {
        self.and_then(|mut proxy| {
            proxy.flag.set(flag, true);
            Ok(proxy)
        })
    }

    pub fn bind_addr(self, addr: String) -> Builder {
        self.and_then(|mut proxy| {
            proxy.bind_addr = addr;
            Ok(proxy)
        })
    }

    pub fn bind_port(self, port: u16) -> Builder {
        self.and_then(|mut proxy| {
            proxy.bind_port = port;
            Ok(proxy)
        })
    }

    pub fn server(self, addr: Option<SocketAddr>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.server = addr;
            Ok(proxy)
        })
    }

    pub fn ts(self, is_tls: bool) -> Builder {
        self.and_then(|mut proxy| {
            proxy.ts = is_tls;
            Ok(proxy)
        })
    }

    pub fn proxy(self, p: bool) -> Builder {
        self.and_then(|mut proxy| {
            proxy.proxy = p;
            Ok(proxy)
        })
    }

    pub fn center(self, center: bool) -> Builder {
        self.and_then(|mut proxy| {
            proxy.center = center;
            Ok(proxy)
        })
    }

    pub fn tc(self, is_tls: bool) -> Builder {
        self.and_then(|mut proxy| {
            proxy.tc = is_tls;
            Ok(proxy)
        })
    }

    pub fn cert(self, cert: Option<String>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.cert = cert;
            Ok(proxy)
        })
    }

    pub fn key(self, key: Option<String>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.key = key;
            Ok(proxy)
        })
    }

    pub fn domain(self, domain: Option<String>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.domain = domain;
            Ok(proxy)
        })
    }

    pub fn username(self, username: Option<String>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.username = username;
            Ok(proxy)
        })
    }

    pub fn password(self, password: Option<String>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.password = password;
            Ok(proxy)
        })
    }

    pub fn udp_bind(self, udp_bind: Option<IpAddr>) -> Builder {
        self.and_then(|mut proxy| {
            proxy.udp_bind = udp_bind;
            Ok(proxy)
        })
    }

    fn and_then<F>(self, func: F) -> Self
    where
        F: FnOnce(ProxyOption) -> ProxyResult<ProxyOption>,
    {
        Builder {
            inner: self.inner.and_then(func),
        }
    }
}

/// 代理类, 一个代理类启动一种类型的代理
pub struct ProxyOption {
    pub(crate) flag: Flag,
    pub(crate) bind_addr: String,
    pub(crate) bind_port: u16,
    pub(crate) server: Option<SocketAddr>,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) udp_bind: Option<IpAddr>,

    //// 是否只接收来自代理的连接
    pub(crate) proxy: bool,
    //// 是否启用一对多
    pub(crate) center: bool,
    /// 连接服务端是否启用tls
    pub(crate) ts: bool,
    /// 接收客户端是否启用tls
    pub(crate) tc: bool,
    /// tls证书所用的域名
    pub(crate) domain: Option<String>,
    /// 公开的证书公钥文件
    pub(crate) cert: Option<String>,
    /// 隐私的证书私钥文件
    pub(crate) key: Option<String>,
}

impl Default for ProxyOption {
    fn default() -> Self {
        Self {
            flag: Flag::HTTP | Flag::HTTPS,
            bind_addr: "127.0.0.1".to_string(),
            bind_port: 8090,
            server: None,
            username: None,
            password: None,
            udp_bind: None,

            proxy: false,
            center: false,
            ts: false,
            tc: false,
            domain: None,
            cert: None,
            key: None,
        }
    }
}

impl ProxyOption {
    pub fn builder() -> Builder {
        Builder::new()
    }

    pub fn parse_env() -> ProxyResult<ProxyOption> {
        let command = Commander::new()
            .version(&env!("CARGO_PKG_VERSION").to_string())
            .usage("-b 127.0.0.1 -p 8090")
            .usage_desc("wmproxy -p 8090")
            .option_list(
                "-f, --flag [value]",
                "可兼容的方法, 如http https socks5",
                None,
            )
            .option("--proxy value", "是否只接收来自代理的连接", Some(false))
            .option("-c, --center value", "是否启用一对多", Some(false))
            .option("--tc value", "接收客户端是否加密", Some(false))
            .option("--ts value", "连接服务端是否加密", Some(false))
            .option_str("--cert value", "证书的公钥", None)
            .option_str("--key value", "证书的私钥", None)
            .option_str("--domain value", "证书的域名", None)
            .option_int("-p, --port value", "监听端口", Some(8090))
            .option_str(
                "-b, --bind value",
                "监听地址",
                Some("127.0.0.1".to_string()),
            )
            .option_str("--user value", "auth的用户名", None)
            .option_str("-S value", "父级的监听端口地址,如127.0.0.1:8091", None)
            .option_str("--pass value", "auth的密码", None)
            .option_str(
                "--udp value",
                "udp的监听地址,如127.0.0.1,socks5的udp协议用",
                None,
            )
            .parse_env_or_exit();

        let listen_port: u16 = command.get_int("p").unwrap() as u16;
        let listen_host = command.get_str("b").unwrap();
        let mut builder = Self::builder().bind_port(listen_port);
        println!("listener bind {} {}", listen_host, listen_port);
        match format!("{}:{}", listen_host, listen_port).parse::<SocketAddr>() {
            Err(_) => {
                builder = builder.bind_addr("127.0.0.1".to_string());
            }
            Ok(_) => {
                builder = builder.bind_addr(listen_host);
            }
        };
        builder = builder.flag(Flag::HTTP | Flag::HTTPS | Flag::SOCKS5);
        builder = builder.username(command.get_str("user"));
        builder = builder.password(command.get_str("pass"));
        builder = builder.tc(command.get("tc").unwrap_or(false));
        builder = builder.ts(command.get("ts").unwrap_or(false));
        builder = builder.center(command.get("c").unwrap_or(false));
        builder = builder.proxy(command.get("proxy").unwrap_or(false));
        builder = builder.domain(command.get_str("domain"));
        builder = builder.cert(command.get_str("cert"));
        builder = builder.key(command.get_str("key"));
        if let Some(udp) = command.get_str("udp") {
            builder = builder.udp_bind(udp.parse::<IpAddr>().ok());
        };

        if let Some(s) = command.get_str("S") {
            builder = builder.server(s.parse::<SocketAddr>().ok());
        };

        builder.inner
    }

    fn load_certs(path: &Option<String>) -> io::Result<Vec<Certificate>> {
        if let Some(path) = path {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            let certs = rustls_pemfile::certs(&mut reader)?;
            Ok(certs.into_iter().map(Certificate).collect())
        } else {
            let cert = br"-----BEGIN CERTIFICATE-----
MIIF+zCCBOOgAwIBAgIQCkkcvmucB5JXt9JAehuNqTANBgkqhkiG9w0BAQsFADBu
MQswCQYDVQQGEwJVUzEVMBMGA1UEChMMRGlnaUNlcnQgSW5jMRkwFwYDVQQLExB3
d3cuZGlnaWNlcnQuY29tMS0wKwYDVQQDEyRFbmNyeXB0aW9uIEV2ZXJ5d2hlcmUg
RFYgVExTIENBIC0gRzIwHhcNMjMwOTIwMDAwMDAwWhcNMjQwOTIwMjM1OTU5WjAc
MRowGAYDVQQDExFzb2Z0LndtLXByb3h5LmNvbTCCASIwDQYJKoZIhvcNAQEBBQAD
ggEPADCCAQoCggEBAMO4HYTBMKeWpePVeG9w5MD1lGvwImsaOXZPPiVhE2iH1PC/
cOgYimSVkaPDMREn4mboPpJz/rZZZEkw0s3t7u2boGPcLocyA5JrEhz/SCr4CWhh
Kxn26NjTazqjGH7rkTIswLCHht0R8QqVW4n4Ikg9o9xttrucYF8fuJvWsCnWL/FC
TwGvoSa253bxXCJ2jpdhSBSS+BwYfdJHoB1j3+LRiT8/HFA0PSg2e3kjnYRtiSaH
8kJpu5l4MZJaBeYqp7/BzJlX7D7hlgdEnfEXBHFG8JfdpvrgrClzM1TDj0Vjt9Kc
Q2Ye+n4zaJ8/oCkJanXlZSfK8fLL20B+pmhuDYcCAwEAAaOCAuUwggLhMB8GA1Ud
IwQYMBaAFHjfkZBf7t6s9sV169VMVVPvJEq2MB0GA1UdDgQWBBSftBOi929JcrJy
D7WHstVroEbsLjAcBgNVHREEFTATghFzb2Z0LndtLXByb3h5LmNvbTA+BgNVHSAE
NzA1MDMGBmeBDAECATApMCcGCCsGAQUFBwIBFhtodHRwOi8vd3d3LmRpZ2ljZXJ0
LmNvbS9DUFMwDgYDVR0PAQH/BAQDAgWgMB0GA1UdJQQWMBQGCCsGAQUFBwMBBggr
BgEFBQcDAjCBgAYIKwYBBQUHAQEEdDByMCQGCCsGAQUFBzABhhhodHRwOi8vb2Nz
cC5kaWdpY2VydC5jb20wSgYIKwYBBQUHMAKGPmh0dHA6Ly9jYWNlcnRzLmRpZ2lj
ZXJ0LmNvbS9FbmNyeXB0aW9uRXZlcnl3aGVyZURWVExTQ0EtRzIuY3J0MAwGA1Ud
EwEB/wQCMAAwggF/BgorBgEEAdZ5AgQCBIIBbwSCAWsBaQB3AO7N0GTV2xrOxVy3
nbTNE6Iyh0Z8vOzew1FIWUZxH7WbAAABirCUmIAAAAQDAEgwRgIhAPPLhOxe+6gj
ywDOzmDW8dL8MUzb/StZQex5/KZ4zezOAiEA9WCnxbr3P/sJmTSqqtgPBQGIa/st
xg92qmSh9oOsXikAdgBIsONr2qZHNA/lagL6nTDrHFIBy1bdLIHZu7+rOdiEcwAA
AYqwlJiSAAAEAwBHMEUCIHarznxsuYuJ7ErhF+BfpjjLui/eyFBC24JhtqhdHJ1H
AiEAsfRfxg8x+F87dpbAP2VGQxJM0ycbVSk2W4pyX6/get4AdgDatr9rP7W2Ip+b
wrtca+hwkXFsu1GEhTS9pD0wSNf7qwAAAYqwlJhpAAAEAwBHMEUCIBckLfu2WU/4
CL9lT8SptA0WOGExlre6BIOilSU7DJd+AiEA0FhK1/Ar0pBLhW259HXqLOH64n9c
IOvFhW+vpwigZO4wDQYJKoZIhvcNAQELBQADggEBAEjZu3ogsPBZ1m6b4FdvXQ6x
l1DMulPFtjtNU56cFxa1J2C5X08OZililg7uvBUSaqt9TRHNq+SEeyE6YlVVwSbZ
7Jcc4aC/ZEk7m3qHJllQiGt3Br+H1Erhwr9fx0FAW8A7YPxj3QKpp1tjH/wbQR7i
KAGn9FipIhtW68gMBH9OR+2e2lcY24IUrTJ6l47jrK3aq1Izl0SSQqobdqX2hSyx
KSiBV2ZVIpKHuxCtgp4VuNacoVJ9aDHIRZ87UdCB82Trui7oao8B5D7DDl89RqQs
DawEK+lxC9RRlhv6thcVWle8oNX3r0FrfTDcmLm2NhWTOi894QLyDBj7pQ8hKXE=
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIIEqjCCA5KgAwIBAgIQDeD/te5iy2EQn2CMnO1e0zANBgkqhkiG9w0BAQsFADBh
MQswCQYDVQQGEwJVUzEVMBMGA1UEChMMRGlnaUNlcnQgSW5jMRkwFwYDVQQLExB3
d3cuZGlnaWNlcnQuY29tMSAwHgYDVQQDExdEaWdpQ2VydCBHbG9iYWwgUm9vdCBH
MjAeFw0xNzExMjcxMjQ2NDBaFw0yNzExMjcxMjQ2NDBaMG4xCzAJBgNVBAYTAlVT
MRUwEwYDVQQKEwxEaWdpQ2VydCBJbmMxGTAXBgNVBAsTEHd3dy5kaWdpY2VydC5j
b20xLTArBgNVBAMTJEVuY3J5cHRpb24gRXZlcnl3aGVyZSBEViBUTFMgQ0EgLSBH
MjCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAO8Uf46i/nr7pkgTDqnE
eSIfCFqvPnUq3aF1tMJ5hh9MnO6Lmt5UdHfBGwC9Si+XjK12cjZgxObsL6Rg1njv
NhAMJ4JunN0JGGRJGSevbJsA3sc68nbPQzuKp5Jc8vpryp2mts38pSCXorPR+sch
QisKA7OSQ1MjcFN0d7tbrceWFNbzgL2csJVQeogOBGSe/KZEIZw6gXLKeFe7mupn
NYJROi2iC11+HuF79iAttMc32Cv6UOxixY/3ZV+LzpLnklFq98XORgwkIJL1HuvP
ha8yvb+W6JislZJL+HLFtidoxmI7Qm3ZyIV66W533DsGFimFJkz3y0GeHWuSVMbI
lfsCAwEAAaOCAU8wggFLMB0GA1UdDgQWBBR435GQX+7erPbFdevVTFVT7yRKtjAf
BgNVHSMEGDAWgBROIlQgGJXm427mD/r6uRLtBhePOTAOBgNVHQ8BAf8EBAMCAYYw
HQYDVR0lBBYwFAYIKwYBBQUHAwEGCCsGAQUFBwMCMBIGA1UdEwEB/wQIMAYBAf8C
AQAwNAYIKwYBBQUHAQEEKDAmMCQGCCsGAQUFBzABhhhodHRwOi8vb2NzcC5kaWdp
Y2VydC5jb20wQgYDVR0fBDswOTA3oDWgM4YxaHR0cDovL2NybDMuZGlnaWNlcnQu
Y29tL0RpZ2lDZXJ0R2xvYmFsUm9vdEcyLmNybDBMBgNVHSAERTBDMDcGCWCGSAGG
/WwBAjAqMCgGCCsGAQUFBwIBFhxodHRwczovL3d3dy5kaWdpY2VydC5jb20vQ1BT
MAgGBmeBDAECATANBgkqhkiG9w0BAQsFAAOCAQEAoBs1eCLKakLtVRPFRjBIJ9LJ
L0s8ZWum8U8/1TMVkQMBn+CPb5xnCD0GSA6L/V0ZFrMNqBirrr5B241OesECvxIi
98bZ90h9+q/X5eMyOD35f8YTaEMpdnQCnawIwiHx06/0BfiTj+b/XQih+mqt3ZXe
xNCJqKexdiB2IWGSKcgahPacWkk/BAQFisKIFYEqHzV974S3FAz/8LIfD58xnsEN
GfzyIDkH3JrwYZ8caPTf6ZX9M1GrISN8HnWTtdNCH2xEajRa/h9ZBXjUyFKQrGk2
n2hcLrfZSbynEC/pSw/ET7H5nWwckjmAJ1l9fcnbqkU/pf6uMQmnfl0JQjJNSg==
-----END CERTIFICATE-----
            ";

            let cursor = io::Cursor::new(cert);
            let mut buf = BufReader::new(cursor);
            let certs = rustls_pemfile::certs(&mut buf)?;
            Ok(certs.into_iter().map(Certificate).collect())
        }
    }

    fn load_keys(path: &Option<String>) -> io::Result<PrivateKey> {
        let mut keys = if let Some(path) = path {
            let file = File::open(&path)?;
            let mut reader = BufReader::new(file);
            rustls_pemfile::pkcs8_private_keys(&mut reader)?
        } else {
            let key = br"-----BEGIN RSA PRIVATE KEY-----
MIIEpQIBAAKCAQEAw7gdhMEwp5al49V4b3DkwPWUa/Aiaxo5dk8+JWETaIfU8L9w
6BiKZJWRo8MxESfiZug+knP+tllkSTDSze3u7ZugY9wuhzIDkmsSHP9IKvgJaGEr
Gfbo2NNrOqMYfuuRMizAsIeG3RHxCpVbifgiSD2j3G22u5xgXx+4m9awKdYv8UJP
Aa+hJrbndvFcInaOl2FIFJL4HBh90kegHWPf4tGJPz8cUDQ9KDZ7eSOdhG2JJofy
Qmm7mXgxkloF5iqnv8HMmVfsPuGWB0Sd8RcEcUbwl92m+uCsKXMzVMOPRWO30pxD
Zh76fjNonz+gKQlqdeVlJ8rx8svbQH6maG4NhwIDAQABAoIBAAx+3UJECU9cfWPh
6v9bxacSJsiLZiR7Yfl35ApZO8wmhpsGRfbzgNEc1tW4mRHff4NoHCftMvEWckzV
SqguIt0rB0meMvRGZlb7R1vwP9Mfzzie94mqq2wTIhtLcr1rscidIIJEwitWwWfB
ExKTHoaJNO8rrAknWeRzhIL36SW1M793EmsbVatRD+GhogcKcZBevQBcddnaZjSw
1WAjS9LcbbQ1tn8y61dYz6L/2MhvDoDOz5oeVB9SC2RPs7omYvQPUig1tYbFxuDu
ruhaVE3BdAKGayJX6Z5lnEA6N66R1JodVBYTESWlTyEenidGg02HsD000eKcdtr7
0M2BAzECgYEA/bpXxHxMIJN+gL6uerEOeNSXYndJy/Y1GHVvdZP2YpL1eS1EK7RD
UNITpG9J++ulCuUb0jiGgJO0SdvKtZbmEYzysj6rUfbxDUBVSazkzVJUXJXiGB7m
0suGjb3xoCH9BzYofPnKfykmlUHVEVdU7eMOu/IjhoCpiRelKAMmnrECgYEAxXjK
bVigxTaTNyDANFCid7S37VuwlEXFC6mwGPAxyZPB4fXZ8Hti/H4uZ5MYSSjzTgVh
8ZWqHJXFC18KvOiLezhUS5rKwvqi/FlnPL/fk/s4qt8Cv1yExxW243SCLWpNIq4Y
/NIhExA2MF4S+mKtoRuAZZagH4WcE8Qplc8lrbcCgYEA+d406M7vuXUHM4qVEUak
VeImY1XOWwpQJ5Ie/c+E6HaJP5iQdenEESeRKHJgjbL2idAuocwAyUasWcAV1NaS
I96Gc3q8BLAHm2ErnK6jdIALjFIeolpsPlMoYxYXifdu01dGcC0eejPwRzTZu4Yh
oVPmArjmu2KhktyyTMEtm0ECgYEAwSj4iaFKEd7ifehRWlsNsR5bU5h+z2q35kKj
+KDrcoxP+KGt/2gSWX1sEvB1rwqZhFYLim6lqbRuvELJlCO8XFmrSxEtCTB1wXYK
YAgnwO7abXobi+gKEVuSPEe5FoeG0EeQNa2toKIY/5Ll6Xog8Rifrb96/ZqKI2Oc
cefgqV0CgYEAy6E0iLoE/59Us8YlE5h+MhX1ktUSFVndkE0vHeLQxOCOmeHWUDhg
cR+nZ6DRmzKISbcN9/m8I7xNWwU2cglrYa4NCHguQSrTefhRoZAfl8BEOW1rJVGC
9D/+Pt3qW+t6dfM5doK7Eb9+nyGiNw/G0/VywTlxf48YKntBbPi8Uww=
-----END RSA PRIVATE KEY-----
            ";
            let cursor = io::Cursor::new(key);
            let mut buf = BufReader::new(cursor);
            // rustls_pemfile::pkcs8_private_keys(&mut buf)?
            rustls_pemfile::rsa_private_keys(&mut buf)?
        };

        match keys.len() {
            0 => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("No PKCS8-encoded private key found"),
            )),
            1 => Ok(PrivateKey(keys.remove(0))),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("More than one PKCS8-encoded private key found"),
            )),
        }
    }

    /// 获取服务端https的证书信息
    pub async fn get_tls_accept(&mut self) -> ProxyResult<TlsAcceptor> {
        if !self.tc {
            return Err(ProxyError::ProtNoSupport);
        }
        let certs = Self::load_certs(&self.cert)?;
        let key = Self::load_keys(&self.key)?;

        let config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|err| {
                println!("error = {:?}", err);
                io::Error::new(io::ErrorKind::InvalidInput, err)
            })?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        Ok(acceptor)
    }

    /// 获取客户端https的Config配置
    pub async fn get_tls_request(&mut self) -> ProxyResult<Arc<rustls::ClientConfig>> {
        if !self.ts {
            return Err(ProxyError::ProtNoSupport);
        }
        let certs = Self::load_certs(&self.cert)?;
        let mut root_cert_store = rustls::RootCertStore::empty();
        for cert in certs {
            let _ = root_cert_store.add(&cert);
        }
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        Ok(Arc::new(config))
    }
}