use crate::{
    html::attribute::Attribute,
    hydration::Cursor,
    renderer::{CastFrom, Renderer},
    ssr::StreamBuilder,
    view::{
        add_attr::AddAnyAttr, Mountable, Position, PositionState, Render,
        RenderHtml, ToTemplate,
    },
};
use const_str_slice_concat::{
    const_concat, const_concat_with_prefix, str_from_buffer,
};
use next_tuple::NextTuple;
use std::{marker::PhantomData, ops::Deref};

mod custom;
mod elements;
mod inner_html;
use super::attribute::{escape_attr, NextAttribute};
pub use custom::*;
pub use elements::*;
pub use inner_html::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HtmlElement<E, At, Ch, Rndr> {
    pub(crate) tag: E,
    pub(crate) rndr: PhantomData<Rndr>,
    pub(crate) attributes: At,
    pub(crate) children: Ch,
    #[cfg(debug_assertions)]
    pub(crate) defined_at: &'static std::panic::Location<'static>,
}

/*impl<E, At, Ch, Rndr> ElementType for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType,
{
    type Output = E::Output;

    const TAG: &'static str = E::TAG;

    const SELF_CLOSING: bool = E::SELF_CLOSING;

    fn tag(&self) -> &str {
        Self::TAG
    }
}*/

impl<E, At, Ch, Rndr> HtmlElement<E, At, Ch, Rndr> {
    pub fn children(&self) -> &Ch {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut Ch {
        &mut self.children
    }

    pub fn attributes(&self) -> &At {
        &self.attributes
    }

    pub fn attributes_mut(&mut self) -> &mut At {
        &mut self.attributes
    }
}

impl<E, At, Ch, NewChild, Rndr> ElementChild<Rndr, NewChild>
    for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementWithChildren,
    Ch: Render<Rndr> + NextTuple,
    <Ch as NextTuple>::Output<NewChild>: Render<Rndr>,
    Rndr: Renderer,
    NewChild: Render<Rndr>,
{
    type Output = HtmlElement<E, At, <Ch as NextTuple>::Output<NewChild>, Rndr>;

    fn child(self, child: NewChild) -> Self::Output {
        let HtmlElement {
            tag,
            rndr,
            attributes,
            children,
            #[cfg(debug_assertions)]
            defined_at,
        } = self;
        HtmlElement {
            tag,
            rndr,
            attributes,
            children: children.next_tuple(child),
            #[cfg(debug_assertions)]
            defined_at,
        }
    }
}

impl<E, At, Ch, Rndr> AddAnyAttr<Rndr> for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType + CreateElement<Rndr> + Send,
    At: Attribute<Rndr> + Send,
    Ch: RenderHtml<Rndr> + Send,
    Rndr: Renderer,
{
    type Output<SomeNewAttr: Attribute<Rndr>> = HtmlElement<
        E,
        <At as NextAttribute<Rndr>>::Output<SomeNewAttr>,
        Ch,
        Rndr,
    >;

    fn add_any_attr<NewAttr: Attribute<Rndr>>(
        self,
        attr: NewAttr,
    ) -> Self::Output<NewAttr> {
        let HtmlElement {
            tag,
            attributes,
            children,
            rndr,
            #[cfg(debug_assertions)]
            defined_at,
        } = self;
        HtmlElement {
            tag,
            attributes: attributes.add_any_attr(attr),
            children,
            rndr,
            #[cfg(debug_assertions)]
            defined_at,
        }
    }
}

pub trait ElementChild<Rndr, NewChild>
where
    NewChild: Render<Rndr>,
    Rndr: Renderer,
{
    type Output;

    fn child(self, child: NewChild) -> Self::Output;
}

pub trait ElementType: Send {
    /// The underlying native widget type that this represents.
    type Output;

    const TAG: &'static str;
    const SELF_CLOSING: bool;
    const ESCAPE_CHILDREN: bool;

    fn tag(&self) -> &str;
}

pub trait HasElementType {
    type ElementType;
}

pub trait ElementWithChildren {}

pub trait CreateElement<R: Renderer> {
    fn create_element(&self) -> R::Element;
}

impl<E, At, Ch, Rndr> HasElementType for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType,
{
    type ElementType = E::Output;
}

impl<E, At, Ch, Rndr> Render<Rndr> for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType + CreateElement<Rndr>,
    At: Attribute<Rndr>,
    Ch: Render<Rndr>,
    Rndr: Renderer,
{
    type State = ElementState<At::State, Ch::State, Rndr>;

    fn rebuild(self, state: &mut Self::State) {
        let ElementState {
            attrs, children, ..
        } = state;
        self.attributes.rebuild(attrs);
        if let Some(children) = children {
            self.children.rebuild(children);
        }
    }

    fn build(self) -> Self::State {
        let el = Rndr::create_element(self.tag);

        let attrs = self.attributes.build(&el);
        let children = if E::SELF_CLOSING {
            None
        } else {
            let mut children = self.children.build();
            children.mount(&el, None);
            Some(children)
        };
        ElementState {
            el,
            attrs,
            children,
            rndr: PhantomData,
        }
    }
}

impl<E, At, Ch, Rndr> RenderHtml<Rndr> for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType + CreateElement<Rndr> + Send,
    At: Attribute<Rndr> + Send,
    Ch: RenderHtml<Rndr> + Send,
    Rndr: Renderer,
{
    type AsyncOutput = HtmlElement<E, At, Ch::AsyncOutput, Rndr>;

    const MIN_LENGTH: usize = if E::SELF_CLOSING {
        3 // < ... />
        + E::TAG.len()
        + At::MIN_LENGTH
    } else {
        2 // < ... >
        + E::TAG.len()
        + At::MIN_LENGTH
        + Ch::MIN_LENGTH
        + 3 // </ ... >
        + E::TAG.len()
    };

    fn dry_resolve(&mut self) {
        self.children.dry_resolve()
    }

    async fn resolve(self) -> Self::AsyncOutput {
        HtmlElement {
            tag: self.tag,
            rndr: PhantomData,
            attributes: self.attributes,
            children: self.children.resolve().await,
            #[cfg(debug_assertions)]
            defined_at: self.defined_at,
        }
    }

    fn html_len(&self) -> usize {
        if E::SELF_CLOSING {
            3 // < ... />
        + E::TAG.len()
        + self.attributes.html_len()
        } else {
            2 // < ... >
        + E::TAG.len()
        + self.attributes.html_len()
        + self.children.html_len()
        + 3 // </ ... >
        + E::TAG.len()
        }
    }

    fn to_html_with_buf(
        self,
        buf: &mut String,
        position: &mut Position,
        _escape: bool,
    ) {
        // opening tag
        buf.push('<');
        buf.push_str(E::TAG);

        // attributes

        // `class` and `style` are created first, and pushed later
        // this is because they can be filled by a mixture of values that include
        // either the whole value (`class="..."` or `style="..."`) and individual
        // classes and styles (`class:foo=true` or `style:height="40px"`), so they
        // need to be filled during the whole attribute-creation process and then
        // added

        // String doesn't allocate until the first push, so this is cheap if there
        // is no class or style on an element
        let mut class = String::new();
        let mut style = String::new();
        let mut inner_html = String::new();

        // inject regular attributes, and fill class and style
        self.attributes
            .to_html(buf, &mut class, &mut style, &mut inner_html);

        if !class.is_empty() {
            buf.push(' ');
            buf.push_str("class=\"");
            buf.push_str(&escape_attr(class.trim_start().trim_end()));
            buf.push('"');
        }
        if !style.is_empty() {
            buf.push(' ');
            buf.push_str("style=\"");
            buf.push_str(&escape_attr(style.trim_start().trim_end()));
            buf.push('"');
        }

        buf.push('>');

        if !E::SELF_CLOSING {
            if !inner_html.is_empty() {
                buf.push_str(&inner_html);
            } else {
                // children
                *position = Position::FirstChild;
                self.children.to_html_with_buf(
                    buf,
                    position,
                    E::ESCAPE_CHILDREN,
                );
            }

            // closing tag
            buf.push_str("</");
            buf.push_str(E::TAG);
            buf.push('>');
        }
        *position = Position::NextChild;
    }

    fn to_html_async_with_buf<const OUT_OF_ORDER: bool>(
        self,
        buffer: &mut StreamBuilder,
        position: &mut Position,
        _escape: bool,
    ) where
        Self: Sized,
    {
        let mut buf = String::with_capacity(Self::MIN_LENGTH);
        // opening tag
        buf.push('<');
        buf.push_str(E::TAG);

        // attributes

        // `class` and `style` are created first, and pushed later
        // this is because they can be filled by a mixture of values that include
        // either the whole value (`class="..."` or `style="..."`) and individual
        // classes and styles (`class:foo=true` or `style:height="40px"`), so they
        // need to be filled during the whole attribute-creation process and then
        // added

        // String doesn't allocate until the first push, so this is cheap if there
        // is no class or style on an element
        let mut class = String::new();
        let mut style = String::new();
        let mut inner_html = String::new();

        // inject regular attributes, and fill class and style
        self.attributes.to_html(
            &mut buf,
            &mut class,
            &mut style,
            &mut inner_html,
        );

        if !class.is_empty() {
            buf.push(' ');
            buf.push_str("class=\"");
            buf.push_str(class.trim_start().trim_end());
            buf.push('"');
        }
        if !style.is_empty() {
            buf.push(' ');
            buf.push_str("style=\"");
            buf.push_str(style.trim_start().trim_end());
            buf.push('"');
        }

        buf.push('>');
        buffer.push_sync(&buf);

        if !E::SELF_CLOSING {
            // children
            *position = Position::FirstChild;
            if !inner_html.is_empty() {
                buffer.push_sync(&inner_html);
            } else {
                self.children.to_html_async_with_buf::<OUT_OF_ORDER>(
                    buffer,
                    position,
                    E::ESCAPE_CHILDREN,
                );
            }

            // closing tag
            let mut buf = String::with_capacity(3 + E::TAG.len());
            buf.push_str("</");
            buf.push_str(E::TAG);
            buf.push('>');
            buffer.push_sync(&buf);
        }
        *position = Position::NextChild;
    }

    fn hydrate<const FROM_SERVER: bool>(
        self,
        cursor: &Cursor<Rndr>,
        position: &PositionState,
    ) -> Self::State {
        // non-Static custom elements need special support in templates
        // because they haven't been inserted type-wise
        if E::TAG.is_empty() && !FROM_SERVER {
            todo!()
        }

        let curr_position = position.get();
        if curr_position == Position::FirstChild {
            cursor.child();
        } else if curr_position != Position::Current {
            cursor.sibling();
        }
        let el = Rndr::Element::cast_from(cursor.current()).unwrap();

        let attrs = self.attributes.hydrate::<FROM_SERVER>(&el);

        // hydrate children
        let children = if E::SELF_CLOSING {
            None
        } else {
            position.set(Position::FirstChild);
            Some(self.children.hydrate::<FROM_SERVER>(cursor, position))
        };

        // go to next sibling
        cursor.set(el.as_ref().clone());
        position.set(Position::NextChild);

        ElementState {
            el,
            attrs,
            children,
            rndr: PhantomData,
        }
    }
}

pub struct ElementState<At, Ch, R: Renderer> {
    pub el: R::Element,
    pub attrs: At,
    pub children: Option<Ch>,
    rndr: PhantomData<R>,
}

impl<At, Ch, R: Renderer> Deref for ElementState<At, Ch, R> {
    type Target = R::Element;

    fn deref(&self) -> &Self::Target {
        &self.el
    }
}

impl<At, Ch, R> Mountable<R> for ElementState<At, Ch, R>
where
    R: Renderer,
{
    fn unmount(&mut self) {
        R::remove(self.el.as_ref());
    }

    fn mount(&mut self, parent: &R::Element, marker: Option<&R::Node>) {
        R::insert_node(parent, self.el.as_ref(), marker);
    }

    fn insert_before_this(&self, child: &mut dyn Mountable<R>) -> bool {
        if let Some(parent) = R::get_parent(self.el.as_ref()) {
            if let Some(element) = R::Element::cast_from(parent) {
                child.mount(&element, Some(self.el.as_ref()));
                return true;
            }
        }
        false
    }
}

impl<E, At, Ch, Rndr> ToTemplate for HtmlElement<E, At, Ch, Rndr>
where
    E: ElementType,
    At: Attribute<Rndr> + ToTemplate,
    Ch: Render<Rndr> + ToTemplate,
    Rndr: Renderer,
{
    const TEMPLATE: &'static str = str_from_buffer(&const_concat(&[
        "<",
        E::TAG,
        At::TEMPLATE,
        str_from_buffer(&const_concat_with_prefix(
            &[At::CLASS],
            " class=\"",
            "\"",
        )),
        str_from_buffer(&const_concat_with_prefix(
            &[At::STYLE],
            " style=\"",
            "\"",
        )),
        ">",
        Ch::TEMPLATE,
        "</",
        E::TAG,
        ">",
    ]));

    #[allow(unused)] // the variables `class` and `style` might be used, but only with `nightly` feature
    fn to_template(
        buf: &mut String,
        class: &mut String,
        style: &mut String,
        inner_html: &mut String,
        position: &mut Position,
    ) {
        // for custom elements without type known at compile time, do nothing
        if !E::TAG.is_empty() {
            // opening tag and attributes
            let mut class = String::new();
            let mut style = String::new();
            let mut inner_html = String::new();

            buf.push('<');
            buf.push_str(E::TAG);
            <At as ToTemplate>::to_template(
                buf,
                &mut class,
                &mut style,
                &mut inner_html,
                position,
            );

            if !class.is_empty() {
                buf.push(' ');
                buf.push_str("class=\"");
                buf.push_str(class.trim_start().trim_end());
                buf.push('"');
            }
            if !style.is_empty() {
                buf.push(' ');
                buf.push_str("style=\"");
                buf.push_str(style.trim_start().trim_end());
                buf.push('"');
            }
            buf.push('>');

            // children
            *position = Position::FirstChild;
            class.clear();
            style.clear();
            inner_html.clear();
            Ch::to_template(
                buf,
                &mut class,
                &mut style,
                &mut inner_html,
                position,
            );

            // closing tag
            buf.push_str("</");
            buf.push_str(E::TAG);
            buf.push('>');
            *position = Position::NextChild;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{main, p, HtmlElement};
    use crate::{
        html::{
            attribute::global::GlobalAttributes,
            element::{em, ElementChild, Main},
        },
        renderer::mock_dom::MockDom,
        view::{Render, RenderHtml},
    };

    #[test]
    fn mock_dom_creates_element() {
        let el: HtmlElement<Main, _, _, MockDom> =
            main().child(p().id("test").lang("en").child("Hello, world!"));
        let el = el.build();
        assert_eq!(
            el.el.to_debug_html(),
            "<main><p lang=\"en\" id=\"test\">Hello, world!</p></main>"
        );
    }

    #[test]
    fn mock_dom_creates_element_with_several_children() {
        let el: HtmlElement<Main, _, _, MockDom> = main().child(p().child((
            "Hello, ",
            em().child("beautiful"),
            " world!",
        )));
        let el = el.build();
        assert_eq!(
            el.el.to_debug_html(),
            "<main><p>Hello, <em>beautiful</em> world!</p></main>"
        );
    }

    #[cfg(feature = "nightly")]
    #[test]
    fn html_render_allocates_appropriate_buffer() {
        use crate::view::static_types::Static;

        let el: HtmlElement<Main, _, _, MockDom> = main().child(p().child((
            Static::<"Hello, ">,
            em().child(Static::<"beautiful">),
            Static::<" world!">,
        )));
        let allocated_len = el.html_len();
        let html = el.to_html();
        assert_eq!(
            html,
            "<main><p>Hello, <em>beautiful</em> world!</p></main>"
        );
        assert_eq!(html.len(), allocated_len);
    }
}
